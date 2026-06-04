// spp.rs – Nothing / CMF earbuds SPP protocol (reverse-engineered from ear-web)
//
// Protocol (from ear-web/res/js/bluetooth_socket.js):
//   Frame:  [0x55, 0x60, 0x01, CMD_LO, CMD_HI, PAYLOAD_LEN, 0x00, OP_ID, ...payload, CRC_LO, CRC_HI]
//   CRC:    CRC-16/IBM  (poly 0xA001, init 0xFFFF)
//   Battery request command: 49159 (0xC007)
//   Battery response triggered by command: 57345 (0xE001) or 16391 (0x4007)
//   Battery response layout:
//     byte[8]         = number of connected sub-devices
//     byte[9 + i*2]   = device id  (0x02=left, 0x03=right, 0x04=case)
//     byte[10 + i*2]  = battery    (bits 6-0 = level %, bit 7 = is_charging)

use log::{info, warn};
use std::io::{Read, Write};
use std::time::Duration;

// ── Public battery result ────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct BatteryInfo {
    pub left:            Option<u8>,
    pub right:           Option<u8>,
    pub case:            Option<u8>,
    pub left_charging:   bool,
    pub right_charging:  bool,
    pub case_charging:   bool,
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
    frame.push((crc >> 8) as u8);
    frame
}

// ── Response parser ──────────────────────────────────────────────────────────

fn parse_battery(data: &[u8]) -> Option<BatteryInfo> {
    // Minimum: 8-byte header + 1 (num_devices) + at least 2 per device
    if data.len() < 10 || data[0] != 0x55 { return None; }

    // Log the command bytes to understand what the device sends
    let cmd = u16::from_le_bytes([data[3], data[4]]);
    info!("SPP: frame cmd=0x{:04X} ({}) len={}", cmd, cmd, data.len());

    // Verify this is a battery response (commands 0xE001=57345 or 0x4007=16391)
    if cmd != 57345 && cmd != 16391 {
        return None;
    }

    let num_devices = data[8] as usize;
    if data.len() < 9 + num_devices * 2 { return None; }

    const BATTERY_MASK:  u8 = 0x7F;
    const CHARGING_MASK: u8 = 0x80;

    let mut info = BatteryInfo::default();
    for i in 0..num_devices {
        let device_id    = data[9  + i * 2];
        let battery_byte = data[10 + i * 2];
        let level    = battery_byte & BATTERY_MASK;
        let charging = (battery_byte & CHARGING_MASK) != 0;
        match device_id {
            0x02 => { info.left  = Some(level); info.left_charging  = charging; }
            0x03 => { info.right = Some(level); info.right_charging = charging; }
            0x04 => { info.case  = Some(level); info.case_charging  = charging; }
            _    => {}
        }
    }
    Some(info)
}

// ── COM port discovery ───────────────────────────────────────────────────────
//
// Nothing/CMF SPP devices register a virtual COM port under
// "Ports (COM & LPT)" whose FriendlyName contains the device name
// e.g. "CMF Buds 2a 'Dev B' (COM4)".
// We query PnP via PowerShell, extract the COM number, and return it.

#[cfg(target_os = "windows")]
fn find_spp_com_port(device_name: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;
    // Escape single-quotes in device name for PowerShell
    let safe_name = device_name.replace('\'', "''");
    let script = format!(
        r#"$dev = Get-PnpDevice -Class Bluetooth | Where-Object {{ $_.FriendlyName -like '*{safe_name}*' }} | Select-Object -First 1;
        if ($dev -and $dev.InstanceId -match 'DEV_([0-9A-Fa-f]{{12}})') {{
            $mac = $Matches[1];
            $port = Get-PnpDevice -Class Ports -Status OK | Where-Object {{ $_.InstanceId -like "*$mac*" }} | Select-Object -First 1;
            if ($port -and $port.FriendlyName -match '(COM\d+)') {{
                $Matches[1]
            }}
        }}"#
    );
    let mut command = std::process::Command::new("powershell");
    command.creation_flags(0x08000000);
    let out = command
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s.lines().next()?.trim().to_string()) }
}

#[cfg(not(target_os = "windows"))]
fn find_spp_com_port(_device_name: &str) -> Option<String> { None }

// ── Public API ───────────────────────────────────────────────────────────────

/// Open the SPP serial port for `device_name`, send a battery request,
/// wait for a valid response, and return the parsed battery levels.
/// Returns `None` if the device is not found or doesn't respond in time.
pub fn read_battery(device_name: &str) -> Option<BatteryInfo> {
    let com = find_spp_com_port(device_name)?;
    info!("SPP: found port {com} for \"{device_name}\"");

    let mut port = serialport::new(&com, 9600)
        .timeout(Duration::from_millis(1500))  // short per-read timeout so we can retry
        .open()
        .map_err(|e| warn!("SPP: cannot open {com}: {e}"))
        .ok()?;

    // Let the port settle after open
    std::thread::sleep(Duration::from_millis(300));

    let packet = build_battery_request();
    let mut rx_buf: Vec<u8> = Vec::new();
    let mut buf = [0u8; 512];

    // Retry up to 4 times: send request, wait up to 2s for reply
    for attempt in 1..=4u32 {
        info!("SPP: battery request attempt {attempt} → {:02X?}", packet);
        if let Err(e) = port.write_all(&packet) {
            warn!("SPP: write error on attempt {attempt}: {e}");
            continue;
        }
        // Flush write buffer
        let _ = port.flush();

        let attempt_deadline = std::time::Instant::now() + Duration::from_secs(2);
        rx_buf.clear();

        loop {
            if std::time::Instant::now() > attempt_deadline {
                warn!("SPP: attempt {attempt} timed out, retrying…");
                break; // retry the whole send
            }
            match port.read(&mut buf) {
                Ok(n) if n > 0 => {
                    rx_buf.extend_from_slice(&buf[..n]);
                    info!("SPP: received {:02X?} (buf={}B)", &buf[..n], rx_buf.len());
                    // Scan for 0x55 frame start in accumulated buffer
                    let mut search_start = 0;
                    while search_start < rx_buf.len() {
                        if let Some(rel_pos) = rx_buf[search_start..].iter().position(|&b| b == 0x55) {
                            let pos = search_start + rel_pos;
                            let frame = &rx_buf[pos..];
                            if frame.len() < 10 { break; } // wait for more data
                            if let Some(info) = parse_battery(frame) {
                                info!("SPP: battery parsed ✓ {:?}", info);
                                return Some(info);
                            }
                            // Frame command didn't match — skip past this 0x55
                            search_start = pos + 1;
                        } else {
                            break;
                        }
                    }
                }
                Ok(_) => {} // 0 bytes, loop again
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                    warn!("SPP: attempt {attempt} read timeout, retrying…");
                    break; // retry send
                }
                Err(e) => {
                    warn!("SPP: read error: {e}");
                    return None;
                }
            }
        }
        // Brief pause before retry
        std::thread::sleep(Duration::from_millis(200));
    }
    warn!("SPP: all attempts exhausted, no battery response");
    None
}
