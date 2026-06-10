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

// CMF / Nothing proprietary SPP UUID
const SPP_UUID: &str = "aeac4a03-dff5-498f-843a-34487cf133eb";

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

fn try_parse_battery(buf: &[u8]) -> Option<BatteryInfo> {
    let mut i = 0;
    while i < buf.len() {
        if buf[i] != 0x55 {
            i += 1;
            continue;
        }
        // Need at least 10 bytes to inspect header
        if buf.len() - i < 10 {
            break;
        }
        let frame = &buf[i..];
        let cmd = u16::from_le_bytes([frame[3], frame[4]]);
        let payload_len = frame[5] as usize;

        info!("SPP: frame at offset {} cmd=0x{:04X} payload_len={}", i, cmd, payload_len);

        // Battery response commands: 0xE001 (57345) or 0x4007 (16391)
        if cmd != 57345 && cmd != 16391 {
            i += 1; // not a battery frame — skip
            continue;
        }

        // Ensure the full payload is present
        let total_len = 8 + payload_len + 2; // header(8) + payload + crc(2)
        if frame.len() < total_len.max(11) {
            break; // need more data
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
                _    => { warn!("SPP: unknown device_id 0x{:02X}", device_id); }
            }
        }
        info!("SPP: battery parsed ✓ {:?}", info);
        return Some(info);
    }
    None
}

// ── Device MAC discovery ─────────────────────────────────────────────────────

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

/// Connect to the CMF Buds 2a via WinRT RFCOMM, send a battery request,
/// collect the response, and return parsed battery levels.
#[cfg(target_os = "windows")]
pub fn read_battery(device_name: &str) -> Option<BatteryInfo> {
    use windows::{
        Devices::Bluetooth::BluetoothDevice,
        Devices::Bluetooth::Rfcomm::RfcommServiceId,
        Networking::Sockets::StreamSocket,
        Storage::Streams::{DataReader, DataWriter, InputStreamOptions},
        core::GUID,
    };

    let mac = find_device_mac(device_name)?;
    info!("SPP: found MAC {:012X} for \"{}\"", mac, device_name);

    // Get BluetoothDevice by MAC address
    let bt_device = match BluetoothDevice::FromBluetoothAddressAsync(mac)
        .and_then(|op| op.get())
    {
        Ok(d) => d,
        Err(e) => { warn!("SPP: BluetoothDevice lookup failed: {e}"); return None; }
    };

    info!("SPP: device \"{}\" status={:?}", 
          bt_device.Name().unwrap_or_default(), 
          bt_device.ConnectionStatus());

    // Get RFCOMM service by custom UUID
    let uuid = GUID::from(SPP_UUID);
    let svc_id = match RfcommServiceId::FromUuid(uuid) {
        Ok(s) => s,
        Err(e) => { warn!("SPP: RfcommServiceId failed: {e}"); return None; }
    };

    let result = match bt_device.GetRfcommServicesForIdAsync(&svc_id)
        .and_then(|op| op.get())
    {
        Ok(r) => r,
        Err(e) => { warn!("SPP: GetRfcommServicesForId failed: {e}"); return None; }
    };

    let services = match result.Services() {
        Ok(s) => s,
        Err(e) => { warn!("SPP: Services() failed: {e}"); return None; }
    };

    if services.Size().unwrap_or(0) == 0 {
        warn!("SPP: no RFCOMM service found for UUID {SPP_UUID}");
        return None;
    }

    let svc = match services.GetAt(0) {
        Ok(s) => s,
        Err(e) => { warn!("SPP: GetAt(0) failed: {e}"); return None; }
    };

    info!("SPP: connecting to RFCOMM service…");

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
        warn!("SPP: socket connect failed: {e}");
        return None;
    }

    info!("SPP: socket connected!");

    // Brief settle before writing
    std::thread::sleep(std::time::Duration::from_millis(300));

    // Send battery request
    let packet = build_battery_request();
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

    info!("SPP: sent battery request {:02X?}", packet);

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

            if let Some(info) = try_parse_battery(&all_bytes) {
                let _ = socket.Close();
                return Some(info);
            }
        } else {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    let _ = socket.Close();
    warn!("SPP: no battery response. Raw received: {:02X?}", all_bytes);
    None
}

#[cfg(not(target_os = "windows"))]
pub fn read_battery(_device_name: &str) -> Option<BatteryInfo> { None }
