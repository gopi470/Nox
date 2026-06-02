"""
audio_monitor.py
────────────────
Detects whether audio is actively being rendered to the target Bluetooth
device using Windows Core Audio (WASAPI) via comtypes + pycaw.

How it works
────────────
1.  Use IMMDeviceEnumerator to enumerate all ACTIVE render endpoints only.
2.  Cross-reference with AudioUtilities.GetAllDevices() to map device IDs to
    friendly names, then find the one matching our target device.
3.  On that IMMDevice, call Activate(IAudioMeterInformation) to get a live
    peak-level meter.  This gives the instantaneous maximum sample amplitude
    (0.0 = silence, 1.0 = full scale) across all channels.
4.  Poll every POLL_INTERVAL seconds.  If peak > SILENCE_THRESHOLD → playing.
5.  5-second hysteresis: audio must be silent for GRACE_CHECKS consecutive
    polls before the state flips from "playing" to "paused" (prevents false
    pauses during track gaps, video buffering, etc.).

Why IAudioMeterInformation?
    • Reads actual digital samples at the hardware level — not app volume/mute.
    • Works system-wide for ANY audio producer (Spotify, YouTube, VLC, Discord,
      games, local players).
    • A media player paused, a muted tab, silence on a stream → peak = 0.

Why IMMDeviceEnumerator (not AudioUtilities.GetAllDevices().Activate)?
    pycaw's AudioDevice wrapper does NOT expose an Activate() method.
    We must obtain the raw IMMDevice pointer from the enumerator and call
    Activate() on it directly.

Bug in original version
    The original code called target_device.Activate(...) on an AudioDevice
    object which has no .Activate() method, causing a silent exception that
    was swallowed, leaving peak permanently at 0.0 → always "paused".

Public interface
    AudioMonitor(device_name, on_play, on_pause)
        .start()      – begin background thread
        .stop()       – stop thread
        .is_playing() -> bool
"""

import threading
import logging
from typing import Callable, Optional
from ctypes import cast, POINTER

import comtypes
import comtypes.client
from comtypes import GUID

from pycaw.pycaw import (
    AudioUtilities,
    IAudioMeterInformation,
    IMMDeviceEnumerator,
    EDataFlow,
    DEVICE_STATE,
)

logger = logging.getLogger("audio_monitor")

POLL_INTERVAL     = 1.0     # seconds between peak-level checks
SILENCE_THRESHOLD = 0.001   # peak amplitude below this → silence
GRACE_CHECKS      = 5       # consecutive silent checks before "pause" fires

# CLSID for the MMDeviceEnumerator COM class
_CLSID_MMDeviceEnumerator = GUID("{BCDE0395-E52F-467C-8E3D-C4579291692E}")


class AudioMonitor:
    def __init__(
        self,
        device_name: str,
        on_play:  Callable[[], None],
        on_pause: Callable[[], None],
    ):
        self._target       = device_name.lower()
        self._on_play      = on_play
        self._on_pause     = on_pause
        self._playing      = False
        self._silent_count = 0
        self._stop_evt     = threading.Event()
        self._thread       = threading.Thread(
            target=self._poll_loop, name="AudioMonitor", daemon=True
        )

    # ── Public API ───────────────────────────────────────────────────────────

    def start(self) -> None:
        logger.info("AudioMonitor starting (target=%r)", self._target)
        self._thread.start()

    def stop(self) -> None:
        self._stop_evt.set()
        self._thread.join(timeout=10)

    def is_playing(self) -> bool:
        return self._playing

    # ── Internal poll loop ────────────────────────────────────────────────────

    def _poll_loop(self) -> None:
        comtypes.CoInitialize()
        try:
            while not self._stop_evt.is_set():
                try:
                    peak = self._get_peak_level()
                    self._evaluate(peak)
                except Exception as exc:
                    logger.debug("Audio poll error: %s", exc)
                    self._evaluate(0.0)
                self._stop_evt.wait(POLL_INTERVAL)
        finally:
            comtypes.CoUninitialize()

    def _evaluate(self, peak: float) -> None:
        """Update playing state with hysteresis."""
        if peak > SILENCE_THRESHOLD:
            self._silent_count = 0
            if not self._playing:
                self._playing = True
                logger.info("Audio PLAYING (peak=%.4f)", peak)
                self._on_play()
        else:
            self._silent_count += 1
            if self._playing and self._silent_count >= GRACE_CHECKS:
                self._playing = False
                logger.info("Audio PAUSED (silent for %d consecutive checks)", self._silent_count)
                self._on_pause()

    # ── WASAPI peak-level query ───────────────────────────────────────────────

    def _get_peak_level(self) -> float:
        """
        Return the current peak audio level (0.0–1.0) for all active render
        endpoints whose friendly name matches the target device.
        Returns the maximum across all matching endpoints (handles devices that
        expose both a "Headphones" and a "Headset" endpoint).

        Uses IMMDeviceEnumerator → IMMDevice.Activate(IAudioMeterInformation)
        which is the correct COM path — NOT pycaw's AudioDevice wrapper.
        """
        # Step 1: build a map of {device_id -> friendly_name} from pycaw's
        # GetAllDevices() which reads property stores correctly.
        try:
            named: dict[str, str] = {}
            for d in AudioUtilities.GetAllDevices():
                if d.id and d.FriendlyName:
                    named[d.id] = d.FriendlyName
        except Exception as exc:
            logger.debug("GetAllDevices failed: %s", exc)
            named = {}

        # Step 2: enumerate only ACTIVE render endpoints via IMMDeviceEnumerator.
        try:
            enumerator = comtypes.CoCreateInstance(
                _CLSID_MMDeviceEnumerator,
                IMMDeviceEnumerator,
                comtypes.CLSCTX_ALL,
            )
            collection = enumerator.EnumAudioEndpoints(
                EDataFlow.eRender.value,
                DEVICE_STATE.ACTIVE.value,
            )
            count = collection.GetCount()
        except Exception as exc:
            logger.debug("EnumAudioEndpoints failed: %s", exc)
            return 0.0

        # Step 3: for each active render endpoint, check if it is our device.
        max_peak = 0.0
        for i in range(count):
            try:
                imm_dev = collection.Item(i)
                dev_id  = imm_dev.GetId()
                name    = named.get(dev_id, "")
                if self._target not in name.lower():
                    continue

                # Step 4: activate IAudioMeterInformation on the raw IMMDevice.
                iface = imm_dev.Activate(
                    IAudioMeterInformation._iid_,
                    comtypes.CLSCTX_ALL,
                    None,
                )
                meter = cast(iface, POINTER(IAudioMeterInformation))
                peak  = meter.GetPeakValue()
                logger.debug("Endpoint %r peak=%.5f", name, peak)
                if peak > max_peak:
                    max_peak = peak

            except Exception as exc:
                logger.debug("Meter read error for endpoint %d: %s", i, exc)

        return max_peak
