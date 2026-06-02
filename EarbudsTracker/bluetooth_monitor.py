"""
bluetooth_monitor.py
────────────────────
Detects whether "CMF Buds 2a" is currently connected using three strategies
ordered from most reliable to least:

Strategy 1 – WASAPI audio endpoint (PRIMARY, most reliable)
    When a BT audio device connects, Windows registers it as an active audio
    render endpoint.  When it disconnects, the endpoint is immediately removed.
    We enumerate only DEVICE_STATE_ACTIVE endpoints — this is a live,
    kernel-maintained list with zero false-positives.

Strategy 2 – WMI Win32_PnPEntity with BTHENUM DeviceID filter
    BTHENUM (Bluetooth Enumerator) DeviceIDs are only present while the
    device is actively connected.  Paired-but-disconnected devices appear
    under BTH\\, not BTHENUM\\.  We filter by both name AND DeviceID prefix
    so stale paired entries are never matched.

Strategy 3 – WMI broad query (last resort)
    Falls back to the name-only query only when strategies 1 and 2 both
    return None (COM/WMI unavailable).  This was the original (buggy)
    behaviour; it is now only used as an absolute last resort on systems
    where COM is broken.

What was WRONG before
─────────────────────
• Strategy B (registry) read BTHENUM / BTH keys that persist for PAIRED
  devices even when they are disconnected — causing permanent false positives.
• Strategy A (WMI) matched on name + ConfigManagerErrorCode==0, which can
  remain 0 for a paired-but-disconnected device on some Windows builds.

Public interface
    BluetoothMonitor(device_name, on_connect, on_disconnect)
        .start()   – begin background polling thread
        .stop()    – stop thread
        .is_connected() -> bool
"""

import threading
import logging
from typing import Callable, Optional

import comtypes
import comtypes.client
from pycaw.pycaw import AudioUtilities, IMMDeviceEnumerator, EDataFlow, DEVICE_STATE

try:
    import wmi as _wmi
    WMI_AVAILABLE = True
except Exception:
    WMI_AVAILABLE = False

logger = logging.getLogger("bt_monitor")

POLL_INTERVAL = 3.0   # seconds between connection checks


class BluetoothMonitor:
    def __init__(
        self,
        device_name: str,
        on_connect: Callable[[], None],
        on_disconnect: Callable[[], None],
    ):
        self._target   = device_name.lower()
        self._on_conn  = on_connect
        self._on_disc  = on_disconnect
        self._connected: Optional[bool] = None   # None = not yet determined
        self._stop_evt = threading.Event()
        self._thread   = threading.Thread(
            target=self._poll_loop, name="BTMonitor", daemon=True
        )
        self._wmi_obj: Optional[object] = None

    # ── Public API ───────────────────────────────────────────────────────────

    def start(self) -> None:
        logger.info("BluetoothMonitor starting (target=%r)", self._target)
        self._thread.start()

    def stop(self) -> None:
        self._stop_evt.set()
        self._thread.join(timeout=10)

    def is_connected(self) -> bool:
        return bool(self._connected)

    # ── Polling loop ─────────────────────────────────────────────────────────

    def _poll_loop(self) -> None:
        # Must initialise COM on this thread before touching any COM objects
        comtypes.CoInitialize()
        if WMI_AVAILABLE:
            try:
                import pythoncom
                pythoncom.CoInitialize()
                self._wmi_obj = _wmi.WMI()
            except Exception as exc:
                logger.warning("WMI init failed: %s", exc)

        try:
            while not self._stop_evt.is_set():
                try:
                    now_connected = self._check_connected()
                except Exception as exc:
                    logger.debug("BT check error: %s", exc)
                    now_connected = self._connected or False

                if now_connected != self._connected:
                    self._connected = now_connected
                    if now_connected:
                        logger.info("Device CONNECTED")
                        self._on_conn()
                    else:
                        logger.info("Device DISCONNECTED")
                        self._on_disc()

                self._stop_evt.wait(POLL_INTERVAL)
        finally:
            comtypes.CoUninitialize()

    def _check_connected(self) -> bool:
        # Strategy 1: WASAPI active endpoint (most reliable, try first)
        result = self._check_audio_endpoint()
        if result is not None:
            logger.debug("Strategy 1 (WASAPI endpoint): %s", result)
            return result

        # Strategy 2: WMI with BTHENUM DeviceID filter
        result = self._check_wmi_bthenum()
        if result is not None:
            logger.debug("Strategy 2 (WMI BTHENUM): %s", result)
            return result

        # Strategy 3: broad WMI name query (last resort)
        result = self._check_wmi_broad()
        if result is not None:
            logger.debug("Strategy 3 (WMI broad): %s", result)
            return result

        # All strategies unavailable — keep previous state
        logger.debug("All strategies unavailable, keeping prior state=%s", self._connected)
        return self._connected or False

    # ── Strategy 1: WASAPI active audio endpoint ──────────────────────────────

    def _check_audio_endpoint(self) -> Optional[bool]:
        """
        Enumerate audio render endpoints and check if the target device
        is present with state == Active.

        Windows adds the audio endpoint when a BT device connects and removes
        it (or marks it NotPresent) when it disconnects — zero false positives.

        pycaw's device.state is an AudioDeviceState enum whose .name is one of:
          'Active', 'Disabled', 'NotPresent', 'Unplugged'
        Only 'Active' means the device is connected and usable.
        """
        try:
            devices = AudioUtilities.GetAllDevices()
            for device in devices:
                if not device.FriendlyName:
                    continue
                if self._target not in device.FriendlyName.lower():
                    continue
                # device.state is an AudioDeviceState enum
                state = device.state
                # Accept both enum comparison and string fallback
                state_name = getattr(state, "name", str(state))
                if "active" in state_name.lower():
                    logger.debug(
                        "WASAPI found active endpoint: %r (state=%s)",
                        device.FriendlyName, state_name
                    )
                    return True
            return False
        except Exception as exc:
            logger.debug("WASAPI endpoint check failed: %s", exc)
            return None

    # ── Strategy 2: WMI – BTHENUM DeviceID only ──────────────────────────────

    def _check_wmi_bthenum(self) -> Optional[bool]:
        """
        Query Win32_PnPEntity where the DeviceID starts with 'BTHENUM'.
        BTHENUM entries only exist while the Bluetooth device is actively
        connected; paired-but-disconnected devices live under BTH\\ instead.
        """
        if self._wmi_obj is None:
            return None
        try:
            # DeviceID LIKE 'BTHENUM%' ensures we only see active BT sessions
            query = (
                "SELECT Name, DeviceID, ConfigManagerErrorCode "
                "FROM Win32_PnPEntity "
                "WHERE DeviceID LIKE 'BTHENUM%'"
            )
            devices = self._wmi_obj.query(query)
            for dev in devices:
                name = (dev.Name or "").lower()
                if self._target in name and dev.ConfigManagerErrorCode == 0:
                    return True
            return False
        except Exception as exc:
            logger.debug("WMI BTHENUM query failed: %s", exc)
            return None

    # ── Strategy 3: broad WMI name query (last resort) ───────────────────────

    def _check_wmi_broad(self) -> Optional[bool]:
        """
        Last-resort name-only WMI query.  Less reliable (can match paired
        devices on some builds) but better than returning None indefinitely.
        """
        if self._wmi_obj is None:
            return None
        try:
            query = (
                "SELECT Name, ConfigManagerErrorCode "
                "FROM Win32_PnPEntity "
                f"WHERE Name LIKE '%{self._target}%'"
            )
            devices = self._wmi_obj.query(query)
            for dev in devices:
                if dev.ConfigManagerErrorCode == 0:
                    return True
            return False if devices else None
        except Exception as exc:
            logger.debug("WMI broad query failed: %s", exc)
            return None
