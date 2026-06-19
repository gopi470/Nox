// spp.rs – Nothing / CMF earbuds SPP protocol via WinRT RFCOMM
//
// The CMF Buds 2a do NOT respond on the generic Windows serial COM port.
// They listen on a proprietary RFCOMM service UUID:
//   aeac4a03-dff5-498f-843a-34487cf133eb
//
// This implementation uses the Windows Runtime (WinRT) Bluetooth RFCOMM APIs
// to connect directly to that UUID, send the battery request frame, and parse
// the response.
//
// Protocol (from ear-web/res/js/bluetooth_socket.js):
//   Frame:  [0x55, 0x60, 0x01, CMD_LO, CMD_HI, PAYLOAD_LEN, 0x00, OP_ID, ...payload, CRC_LO, CRC_HI]
//   CRC:    CRC-16/IBM  (poly 0xA001, init 0xFFFF)
//   Battery request command:  0xC007 (49159)
//   Battery response commands: 0xE001 (57345) or 0x4007 (16391)
//   Battery response layout:
//     byte[8]         = number of connected sub-devices
//     byte[9 + i*2]   = device id  (0x02=left, 0x03=right, 0x04=case)
//     byte[10 + i*2]  = battery    (bits 6-0 = level %, bit 7 = is_charging)

use log::{info, warn};

// ── Public battery result ────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct BatteryInfo {
    pub left:           Option<u8>,
    pub right:          Option<u8>,
    pub case:           Option<u8>,
    pub left_charging:  bool,
    pub right_charging: bool,
    pub case_charging:  bool,
    #[serde(default)]
    pub updated_at:     Option<u64>,
}

// ── CRC-16 / IBM ─────────────────────────────────────────────────────────────

fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= byte as u16;
        for _ in 0..8 {
            crc = if (crc & 1) != 0 { (crc >> 1) ^ 0xA001 } else { crc >> 1 };
        }
    }
    crc
}

// ── Packet builder ───────────────────────────────────────────────────────────

fn build_battery_request() -> Vec<u8> {
    const CMD: u16 = 49159; // 0xC007 – sendBattery
    let mut frame: Vec<u8> = vec![
        0x55, 0x60, 0x01,
        (CMD & 0xFF) as u8,   // 0x07
        (CMD >> 8)   as u8,   // 0xC0
        0x00,                 // payload length = 0
        0x00,
        0x01,                 // operation id
    ];
    let crc = crc16(&frame);
    frame.push((crc & 0xFF) as u8);
    frame.push((crc >> 8)   as u8);
    frame
}

// ── Response parser ──────────────────────────────────────────────────────────
// The device sends an ACK packet first, then the actual battery data packet.
// We scan through accumulated bytes looking for a battery frame.

// ── Response parsers ──────────────────────────────────────────────────────────

fn try_parse_nothing_cmf(buf: &[u8]) -> Option<BatteryInfo> {
    let mut i = 0;
    while i < buf.len() {
        if buf[i] != 0x55 {
            i += 1;
            continue;
        }
        if buf.len() - i < 10 {
            break;
        }
        let frame = &buf[i..];
        let cmd = u16::from_le_bytes([frame[3], frame[4]]);
        let payload_len = frame[5] as usize;

        if cmd != 57345 && cmd != 16391 {
            i += 1;
            continue;
        }

        let total_len = 8 + payload_len + 2;
        if frame.len() < total_len.max(11) {
            break;
        }

        let num_devices = frame[8] as usize;
        if frame.len() < 9 + num_devices * 2 {
            break;
        }

        let mut info = BatteryInfo::default();
        for d in 0..num_devices {
            let device_id    = frame[9  + d * 2];
            let battery_byte = frame[10 + d * 2];
            let level    = battery_byte & 0x7F;
            let charging = (battery_byte & 0x80) != 0;
            match device_id {
                0x02 => { info.left  = Some(level); info.left_charging  = charging; }
                0x03 => { info.right = Some(level); info.right_charging = charging; }
                0x04 => { info.case  = Some(level); info.case_charging  = charging; }
                _    => {}
            }
        }
        info.updated_at = Some(chrono::Local::now().timestamp_millis() as u64);
        return Some(info);
    }
    None
}

fn try_parse_samsung_galaxy(buf: &[u8]) -> Option<BatteryInfo> {
    let mut i = 0;
    while i < buf.len() {
        if buf[i] != 0xFD {
            i += 1;
            continue;
        }
        if buf.len() - i < 10 {
            break;
        }
        let frame = &buf[i..];
        let payload_len = frame[1] as usize;
        let cmd = frame[3];

        if cmd != 0x2F && cmd != 0x61 && cmd != 0x30 && cmd != 0x0C {
            i += 1;
            continue;
        }

        let total_len = 2 + payload_len + 2;
        if frame.len() < total_len {
            break;
        }

        let left = frame[4];
        let right = frame[5];
        
        let case = if payload_len >= 9 {
            let val = frame[8];
            if val <= 100 { Some(val) } else { None }
        } else {
            None
        };

        let mut left_charging = false;
        let mut right_charging = false;
        let mut case_charging = false;
        if payload_len >= 10 {
            let charging_byte = frame[9];
            left_charging = (charging_byte & 0x01) != 0;
            right_charging = (charging_byte & 0x02) != 0;
            case_charging = (charging_byte & 0x04) != 0;
        }

        if left <= 100 || right <= 100 {
            return Some(BatteryInfo {
                left: if left <= 100 { Some(left) } else { None },
                right: if right <= 100 { Some(right) } else { None },
                case,
                left_charging,
                right_charging,
                case_charging,
                updated_at: Some(chrono::Local::now().timestamp_millis() as u64),
            });
        }
        i += 1;
    }
    None
}

fn try_parse_sony(buf: &[u8]) -> Option<BatteryInfo> {
    let mut i = 0;
    while i < buf.len() {
        if buf[i] != 0x0C {
            i += 1;
            continue;
        }
        if buf.len() - i < 8 {
            break;
        }
        let frame = &buf[i..];
        let payload_len = frame[1] as usize;
        
        let total_len = 2 + payload_len + 1;
        if frame.len() < total_len {
            break;
        }

        let battery_val = frame[6];
        let charging_val = frame[7];
        let is_charging = charging_val == 0x01 || charging_val == 0x03;

        if battery_val <= 100 {
            return Some(BatteryInfo {
                left: Some(battery_val),
                right: Some(battery_val),
                case: None,
                left_charging: is_charging,
                right_charging: is_charging,
                case_charging: false,
                updated_at: Some(chrono::Local::now().timestamp_millis() as u64),
            });
        }
        i += 1;
    }
    None
}

fn try_parse_bose(buf: &[u8]) -> Option<BatteryInfo> {
    let mut i = 0;
    while i < buf.len() {
        if buf[i] != 0x01 {
            i += 1;
            continue;
        }
        if buf.len() - i < 7 {
            break;
        }
        let frame = &buf[i..];
        let cmd = frame[3];
        if cmd != 0x09 {
            i += 1;
            continue;
        }

        let left = frame[4];
        let right = frame[5];
        let case = frame[6];

        if left <= 100 || right <= 100 {
            return Some(BatteryInfo {
                left: if left <= 100 { Some(left) } else { None },
                right: if right <= 100 { Some(right) } else { None },
                case: if case <= 100 { Some(case) } else { None },
                left_charging: false,
                right_charging: false,
                case_charging: false,
                updated_at: Some(chrono::Local::now().timestamp_millis() as u64),
            });
        }
        i += 1;
    }
    None
}

fn parse_spp_response(brand: &str, buf: &[u8]) -> Option<BatteryInfo> {
    match brand {
        "nothing_cmf" => try_parse_nothing_cmf(buf),
        "samsung_galaxy" => try_parse_samsung_galaxy(buf),
        "sony" => try_parse_sony(buf),
        "bose" => try_parse_bose(buf),
        _ => None,
    }
}

fn get_brand_spp_config(brand: &str) -> Option<(&'static str, Vec<u8>)> {
    match brand {
        "nothing_cmf" => {
            Some((
                "aeac4a03-dff5-498f-843a-34487cf133eb",
                build_battery_request()
            ))
        }
        "samsung_galaxy" => {
            Some((
                "00001101-0000-1000-8000-00805f9b34fb",
                vec![0xFD, 0x0C, 0x00, 0x2F, 0xC1, 0x00, 0xF2]
            ))
        }
        "sony" => {
            Some((
                "96cc203e-5068-46ad-b32d-e316f5e069ba",
                vec![0x0C, 0x03, 0x01, 0x02, 0x01, 0xA4]
            ))
        }
        "bose" => {
            Some((
                "00000000-deca-fade-deca-deafdecacaff",
                vec![0x01, 0x09, 0x02, 0x00]
            ))
        }
        _ => None,
    }
}

// ── Device MAC discovery ─────────────────────────────────────────────────────

/// Parse a MAC address string in any common format ("AA:BB:CC:DD:EE:FF",
/// "AA-BB-CC-DD-EE-FF", or "AABBCCDDEEFF") into a raw u64 Bluetooth address.
fn parse_mac_str(s: &str) -> Option<u64> {
    let clean: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if clean.len() == 12 {
        u64::from_str_radix(&clean, 16).ok()
    } else {
        None
    }
}

#[cfg(target_os = "windows")]
fn find_device_mac(device_name: &str) -> Option<u64> {
    use std::os::windows::process::CommandExt;
    let script = r#"$dev = Get-PnpDevice -Class Bluetooth | Where-Object { $_.FriendlyName -like ('*' + $env:DEVICE_NAME + '*') } | Select-Object -First 1;
        if ($dev -and $dev.InstanceId -match 'DEV_([0-9A-Fa-f]{12})') {
            $Matches[1]
        }"#;
    let mut command = std::process::Command::new("powershell");
    command.creation_flags(0x08000000);
    let out = command
        .env("DEVICE_NAME", device_name)
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let mac_str = s.lines().next()?.trim().to_string();
    if mac_str.len() == 12 {
        u64::from_str_radix(&mac_str, 16).ok()
    } else {
        None
    }
}

#[cfg(not(target_os = "windows"))]
fn find_device_mac(_device_name: &str) -> Option<u64> { None }

// ── Public API ───────────────────────────────────────────────────────────────

/// Inner function: given a resolved Bluetooth MAC address, connect via WinRT
/// RFCOMM, query using the brand-specific payload/UUID, and parse using the brand-specific parser.
#[cfg(target_os = "windows")]
fn attempt_spp_query(mac: u64, device_name: &str, brand: &str) -> Option<BatteryInfo> {
    use windows::{
        Devices::Bluetooth::BluetoothDevice,
        Devices::Bluetooth::Rfcomm::RfcommServiceId,
        Networking::Sockets::StreamSocket,
        Storage::Streams::{DataReader, DataWriter, InputStreamOptions},
        core::GUID,
    };

    let (uuid_str, packet) = match get_brand_spp_config(brand) {
        Some(cfg) => cfg,
        None => {
            warn!("SPP: brand '{}' has no SPP configuration", brand);
            return None;
        }
    };

    // Get BluetoothDevice by MAC address
    let bt_device = match BluetoothDevice::FromBluetoothAddressAsync(mac)
        .and_then(|op| op.get())
    {
        Ok(d) => d,
        Err(e) => { warn!("SPP: BluetoothDevice lookup failed for {:012X}: {e}", mac); return None; }
    };

    info!("SPP: device \"{}\" status={:?} brand={}",
          bt_device.Name().unwrap_or_default(),
          bt_device.ConnectionStatus(),
          brand);

    // Get RFCOMM service by custom UUID
    let uuid = GUID::from(uuid_str);
    let svc_id = match RfcommServiceId::FromUuid(uuid) {
        Ok(s) => s,
        Err(e) => { warn!("SPP: RfcommServiceId failed for {uuid_str}: {e}"); return None; }
    };

    let result = match bt_device.GetRfcommServicesForIdAsync(&svc_id)
        .and_then(|op| op.get())
    {
        Ok(r) => r,
        Err(e) => { warn!("SPP: GetRfcommServicesForId failed for {uuid_str}: {e}"); return None; }
    };

    let services = match result.Services() {
        Ok(s) => s,
        Err(e) => { warn!("SPP: Services() failed: {e}"); return None; }
    };

    if services.Size().unwrap_or(0) == 0 {
        warn!("SPP: no RFCOMM service found for UUID {uuid_str}");
        return None;
    }

    let svc = match services.GetAt(0) {
        Ok(s) => s,
        Err(e) => { warn!("SPP: GetAt(0) failed: {e}"); return None; }
    };

    info!("SPP: connecting to RFCOMM service for {brand}…");

    // Connect StreamSocket
    let socket = match StreamSocket::new() {
        Ok(s) => s,
        Err(e) => { warn!("SPP: StreamSocket::new failed: {e}"); return None; }
    };

    let host = match svc.ConnectionHostName() {
        Ok(h) => h,
        Err(e) => { warn!("SPP: ConnectionHostName failed: {e}"); return None; }
    };
    let service_name = match svc.ConnectionServiceName() {
        Ok(n) => n,
        Err(e) => { warn!("SPP: ConnectionServiceName failed: {e}"); return None; }
    };

    if let Err(e) = socket.ConnectAsync(&host, &service_name).and_then(|op| op.get()) {
        warn!("SPP: socket connect failed for {brand}: {e}");
        return None;
    }

    info!("SPP: socket connected for {brand}!");

    // Brief settle before writing
    std::thread::sleep(std::time::Duration::from_millis(300));

    // Send battery request
    let output_stream = match socket.OutputStream() {
        Ok(s) => s,
        Err(e) => { warn!("SPP: OutputStream failed: {e}"); return None; }
    };
    let writer = match DataWriter::CreateDataWriter(&output_stream) {
        Ok(w) => w,
        Err(e) => { warn!("SPP: DataWriter failed: {e}"); return None; }
    };

    if let Err(e) = writer.WriteBytes(&packet) {
        warn!("SPP: WriteBytes failed: {e}");
        return None;
    }
    if let Err(e) = writer.StoreAsync().and_then(|op| op.get()) {
        warn!("SPP: StoreAsync failed: {e}");
        return None;
    }
    let _ = writer.DetachStream();

    info!("SPP: sent battery request {:02X?} for {brand}", packet);

    // Read response — collect for up to 3 seconds
    let input_stream = match socket.InputStream() {
        Ok(s) => s,
        Err(e) => { warn!("SPP: InputStream failed: {e}"); return None; }
    };
    let reader = match DataReader::CreateDataReader(&input_stream) {
        Ok(r) => r,
        Err(e) => { warn!("SPP: DataReader failed: {e}"); return None; }
    };

    if let Err(e) = reader.SetInputStreamOptions(InputStreamOptions::Partial) {
        warn!("SPP: SetInputStreamOptions failed: {e}");
    }

    let mut all_bytes: Vec<u8> = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);

    while std::time::Instant::now() < deadline {
        let avail = match reader.LoadAsync(1024).and_then(|op| op.get()) {
            Ok(n) => n,
            Err(e) => { warn!("SPP: LoadAsync failed: {e}"); break; }
        };

        if avail > 0 {
            let mut buf = vec![0u8; avail as usize];
            if let Err(e) = reader.ReadBytes(&mut buf) {
                warn!("SPP: ReadBytes failed: {e}");
                break;
            }
            info!("SPP: read {} bytes: {:02X?}", buf.len(), buf);
            all_bytes.extend_from_slice(&buf);

            if let Some(info) = parse_spp_response(brand, &all_bytes) {
                let _ = socket.Close();
                return Some(info);
            }
        } else {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    let _ = socket.Close();
    warn!("SPP: no battery response from {:012X} ({}). Raw: {:02X?}", mac, device_name, all_bytes);
    None
}

/// Attempt SPP battery query.
/// Resolution order:
///   1. Parse `mac_hint` if provided and non-empty.
///   2. Fall back to discovering the MAC via friendly-name PowerShell query.
///   3. If the first attempt fails AND we originally had a hint, try once more
///      with the freshly discovered MAC (handles stale stored MACs).
#[cfg(target_os = "windows")]
fn read_battery_spp(device_name: &str, mac_hint: Option<&str>, brand: &str) -> Option<BatteryInfo> {
    // AirPods does not support SPP (BLE only)
    if brand == "apple_airpods" {
        return None;
    }

    // Resolve a MAC to try first
    let hinted_mac = mac_hint.and_then(|s| {
        let m = parse_mac_str(s);
        if m.is_none() && !s.trim().is_empty() {
            warn!("SPP: stored mac_address '{}' is not a valid MAC — falling back to name lookup", s);
        }
        m
    });

    let primary_mac = match hinted_mac {
        Some(m) => {
            info!("SPP: using stored MAC {:012X} for \"{}\"", m, device_name);
            m
        }
        None => {
            // No valid hint — discover by name
            let m = find_device_mac(device_name)?;
            info!("SPP: discovered MAC {:012X} for \"{}\" by friendly name", m, device_name);
            m
        }
    };

    // First attempt
    if let Some(info) = attempt_spp_query(primary_mac, device_name, brand) {
        return Some(info);
    }

    // If we used a stored hint and it failed, try discovering fresh via name
    if hinted_mac.is_some() {
        warn!("SPP: stored MAC {:012X} failed — retrying via friendly-name lookup", primary_mac);
        if let Some(fallback_mac) = find_device_mac(device_name) {
            if fallback_mac != primary_mac {
                info!("SPP: fallback MAC {:012X} differs — retrying", fallback_mac);
                return attempt_spp_query(fallback_mac, device_name, brand);
            } else {
                // Same MAC, no point retrying
                return attempt_spp_query(fallback_mac, device_name, brand);
            }
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn read_battery_airpods() -> Option<BatteryInfo> {
    use windows::{
        Devices::Bluetooth::Advertisement::{BluetoothLEAdvertisementWatcher, BluetoothLEAdvertisementReceivedEventArgs},
        Foundation::TypedEventHandler,
    };
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    log::info!("AirPods: starting BLE advertisement watcher...");
    let result_battery = Arc::new(Mutex::new(None::<BatteryInfo>));
    let result_battery_cb = Arc::clone(&result_battery);

    let watcher = match BluetoothLEAdvertisementWatcher::new() {
        Ok(w) => w,
        Err(e) => {
            log::warn!("AirPods: failed to create BLE watcher: {e}");
            return None;
        }
    };

    // Callback when advertisement is received
    let token = watcher.Received(&TypedEventHandler::new(
        move |_sender, args: &Option<BluetoothLEAdvertisementReceivedEventArgs>| {
            if let Some(args) = args {
                if let Ok(ad) = args.Advertisement() {
                    if let Ok(sections) = ad.DataSections() {
                        for section in sections {
                            let data_type = section.DataType().unwrap_or(0);
                            if data_type == 0xFF {
                                if let Ok(data_buffer) = section.Data() {
                                    if let Ok(data_reader) = windows::Storage::Streams::DataReader::FromBuffer(&data_buffer) {
                                        let len = data_reader.UnconsumedBufferLength().unwrap_or(0) as usize;
                                        if len >= 10 {
                                            let mut buf = vec![0u8; len];
                                            if data_reader.ReadBytes(&mut buf).is_ok() {
                                                // Apple Company ID: 0x4C, 0x00
                                                // Protocol ID: 0x07
                                                // Length: 0x19
                                                if buf[0] == 0x4C && buf[1] == 0x00 && buf[2] == 0x07 && buf[3] == 0x19 {
                                                    let right_val = buf[7] & 0x0F;
                                                    let left_val = (buf[8] >> 4) & 0x0F;
                                                    let charging_status = buf[8] & 0x0F;
                                                    let case_val = (buf[9] >> 4) & 0x0F;

                                                    let left = if left_val <= 10 { Some(left_val * 10) } else { None };
                                                    let right = if right_val <= 10 { Some(right_val * 10) } else { None };
                                                    let case = if case_val <= 10 { Some(case_val * 10) } else { None };

                                                    let left_chg = (charging_status & 0x01) != 0;
                                                    let right_chg = (charging_status & 0x02) != 0;
                                                    let case_chg = (charging_status & 0x04) != 0;

                                                    if left.is_some() || right.is_some() || case.is_some() {
                                                        let info = BatteryInfo {
                                                            left,
                                                            left_charging: left_chg,
                                                            right,
                                                            right_charging: right_chg,
                                                            case,
                                                            case_charging: case_chg,
                                                            updated_at: Some(chrono::Local::now().timestamp_millis() as u64),
                                                        };
                                                        let mut res = result_battery_cb.lock().unwrap();
                                                        *res = Some(info);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Ok(())
        }
    ));

    let token = match token {
        Ok(t) => t,
        Err(e) => {
            log::warn!("AirPods: failed to register event handler: {e}");
            return None;
        }
    };

    if let Err(e) = watcher.Start() {
        log::warn!("AirPods: failed to start BLE watcher: {e}");
        return None;
    }

    // Wait for up to 3 seconds for a packet to arrive
    let start_time = Instant::now();
    while start_time.elapsed() < Duration::from_secs(3) {
        {
            let res = result_battery.lock().unwrap();
            if res.is_some() {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    let _ = watcher.Stop();
    let _ = watcher.RemoveReceived(token);

    let final_res = result_battery.lock().unwrap().clone();
    if final_res.is_some() {
        log::info!("AirPods: successfully sniffed battery via BLE: {:?}", final_res);
    } else {
        log::warn!("AirPods: timed out waiting for BLE advertisement packet");
    }
    final_res
}

#[cfg(target_os = "windows")]
fn read_battery_pnp(device_name: &str) -> Option<BatteryInfo> {
    use std::os::windows::process::CommandExt;
    let script = r#"$devs = Get-PnpDevice | Where-Object { $_.FriendlyName -like ('*' + $env:DEVICE_NAME + '*') };
        foreach ($d in $devs) {
            $val = Get-PnpDeviceProperty -InstanceId $d.InstanceId -KeyName "{104EA319-6EE2-4701-BD47-8DDBF425BBE5} 2" -ErrorAction SilentlyContinue;
            if ($val -and $val.Data -ne $null) {
                $val.Data;
                break;
            }
        }"#;
    let mut command = std::process::Command::new("powershell");
    command.creation_flags(0x08000000);
    let out = command
        .env("DEVICE_NAME", device_name)
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let battery_pct = s.lines().next()?.trim().parse::<u8>().ok()?;
    if battery_pct <= 100 {
        Some(BatteryInfo {
            left: Some(battery_pct),
            left_charging: false,
            right: None,
            right_charging: false,
            case: None,
            case_charging: false,
            updated_at: Some(chrono::Local::now().timestamp_millis() as u64),
        })
    } else {
        None
    }
}

/// Public entry point for battery reading.
/// `brand`: profile brand key — e.g. "nothing_cmf", "samsung_galaxy", "generic_other".
/// `mac_address`: optional stored MAC from the device profile.
/// `protocol_mode`: current persisted mode ("auto", "proprietary", "standard").
///
/// Returns `(Option<BatteryInfo>, &'static str)` — the second value is the *effective* method
/// used ("proprietary" | "standard"). The caller should persist this to skip re-discovery.
#[cfg(target_os = "windows")]
pub fn read_battery(
    device_name: &str,
    mac_address: Option<&str>,
    brand: &str,
    protocol_mode: &str,
    is_connected: bool,
) -> (Option<BatteryInfo>, &'static str) {
    // AirPods has its own dedicated handler (passive BLE advertisement sniffing)
    if brand == "apple_airpods" {
        return (read_battery_airpods(), "proprietary");
    }

    // Generic / Other — PnP only, never attempt SPP
    if brand == "generic_other" {
        if !is_connected {
            return (None, "standard");
        }
        return (read_battery_pnp(device_name), "standard");
    }

    // Already discovered in a previous session — skip rediscovery
    if protocol_mode == "proprietary" {
        if !is_connected {
            return (None, "proprietary");
        }
        return (read_battery_spp(device_name, mac_address, brand), "proprietary");
    }
    if protocol_mode == "standard" {
        if !is_connected {
            return (None, "standard");
        }
        return (read_battery_pnp(device_name), "standard");
    }

    // protocol_mode == "auto": first-time discovery.
    // MUST ONLY perform discovery if the device is currently connected!
    if !is_connected {
        return (None, "auto");
    }

    // Try SPP first
    if let Some(info) = read_battery_spp(device_name, mac_address, brand) {
        return (Some(info), "proprietary");
    }
    (read_battery_pnp(device_name), "standard")
}

#[cfg(not(target_os = "windows"))]
pub fn read_battery(
    _device_name: &str,
    _mac_address: Option<&str>,
    _brand: &str,
    _protocol_mode: &str,
    _is_connected: bool,
) -> (Option<BatteryInfo>, &'static str) {
    (None, "standard")
}

