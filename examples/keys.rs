//! Press UP / DOWN / CONFIRM; hold UP+DOWN on the device to leave.
use zectrix_sdk::{Device, Event, OpenOptions};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let device = Device::open(OpenOptions {
        target: std::env::args().nth(1),
        client_name: Some("Rust keys example".into()),
    })
    .await?;
    println!("按设备按键查看事件；同时长按上下键退出。");
    while let Some(event) = device.next_event().await? {
        if let Event::Key { key, action } = event {
            println!("{key:?}: {action:?}");
        } else if matches!(event, Event::Disconnected { .. }) {
            println!("{event:?}");
            break;
        }
    }
    device.close().await?;
    Ok(())
}
