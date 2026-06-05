use windows::{
    Devices::Bluetooth::{BluetoothDevice, BluetoothCacheMode},
    Devices::Bluetooth::Rfcomm::{RfcommServiceId},
    Networking::Sockets::{StreamSocket, SocketProtectionLevel},
    Storage::Streams::{DataReader, DataWriter, InputStreamOptions},
    core::GUID,
};

fn main() -> windows::core::Result<()> {
    println!("=== Rust WinRT RFCOMM Battery Test ===");
    // MAC address from earlier output: 2CBEEE93008F = 49198558019727
    let mac = 49198558019727;
    
    println!("Connecting to MAC: {}", mac);
    let bt_device = BluetoothDevice::FromBluetoothAddressAsync(mac)?.get()?;
    println!("Connected to: {}", bt_device.Name()?);
    
    let uuid = GUID::from("aeac4a03-dff5-498f-843a-34487cf133eb");
    let svc_id = RfcommServiceId::FromUuid(uuid)?;
    
    let services = bt_device.GetRfcommServicesForIdAsync(&svc_id, BluetoothCacheMode::Uncached)?.get()?;
    let svcs = services.Services()?;
    
    if svcs.Size()? == 0 {
        println!("No RFCOMM services found for UUID.");
        return Ok(());
    }
    
    let svc = svcs.GetAt(0)?;
    println!("Found service: {:?}", svc.ServiceId()?.Uuid()?);
    
    let socket = StreamSocket::new()?;
    socket.ConnectAsync(
        &svc.ConnectionHostName()?,
        &svc.ConnectionServiceName()?,
        SocketProtectionLevel::BluetoothEncryptionAllowNullAuthentication
    )?.get()?;
    println!("Socket connected!");
    
    // Request packet
    let packet: [u8; 10] = [0x55, 0x60, 0x01, 0x07, 0xC0, 0x00, 0x00, 0x01, 0xAC, 0xDF];
    let writer = DataWriter::CreateDataWriter(&socket.OutputStream()?)?;
    writer.WriteBytes(&packet)?;
    writer.StoreAsync()?.get()?;
    writer.DetachStream()?;
    println!("Sent battery request: {:02X?}", packet);
    
    let reader = DataReader::CreateDataReader(&socket.InputStream()?)?;
    reader.SetInputStreamOptions(InputStreamOptions::Partial)?;
    
    let mut all_bytes = Vec::new();
    let mut buffer = vec![0u8; 1024];
    
    println!("Waiting for response packets...");
    for i in 0..5 {
        std::thread::sleep(std::time::Duration::from_millis(200));
        let avail = reader.LoadAsync(1024)?.get()?;
        if avail > 0 {
            reader.ReadBytes(&mut buffer[..avail as usize])?;
            all_bytes.extend_from_slice(&buffer[..avail as usize]);
            println!("Read {} bytes on iteration {}", avail, i);
        }
    }
    
    println!("RAW RESPONSE ({} bytes): {:02X?}", all_bytes.len(), all_bytes);
    
    Ok(())
}
