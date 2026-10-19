use zectrix_sdk::{Device, OpenOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let device = Device::open(OpenOptions {
        target: std::env::args().nth(1),
        client_name: Some("Device status example".into()),
    })
    .await?;
    let result = device.status().await;
    device.close().await?;
    let status = result?;
    println!("电量: {:?} %", status.battery_percent);
    println!("正在充电: {:?}", status.charging);
    println!("Wi-Fi RSSI: {:?} dBm", status.wifi_rssi_dbm);
    Ok(())
}
