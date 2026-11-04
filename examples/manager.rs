//! Run with an optional device IP once to seed its remembered manager address.
//! Later runs need no argument: select “设置 → 网络同步 → 连接管理端” on the device.
use zectrix_sdk::{ConnectionRequestListener, Device, Event, OpenOptions};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut listener = ConnectionRequestListener::bind().await?;
    println!("管理端监听 UDP 32343；设备可选择“设置 → 网络同步 → 连接管理端”。");
    let mut device = if let Some(target) = std::env::args().nth(1) {
        Some(
            Device::open(OpenOptions {
                target: Some(target),
                client_name: Some("Zectrix Manager".into()),
            })
            .await?,
        )
    } else {
        None
    };
    loop {
        if let Some(active) = &device {
            tokio::select! {
                request = listener.next_request() => {
                    let request = request?;
                    println!("已有连接，忽略来自 {} 的请求", request.address);
                }
                event = active.next_event() => {
                    match event {
                        Ok(Some(Event::Disconnected { reason })) => {
                            println!("连接已结束：{reason:?}；等待新的显式请求");
                            device = None;
                        }
                        Ok(Some(event)) => println!("{event:?}"),
                        Ok(None) => device = None,
                        Err(error) => { eprintln!("{error}"); device = None; }
                    }
                }
            }
        } else {
            let request = listener.next_request().await?;
            println!("收到 {} 的连接请求", request.address);
            match request.accept("Zectrix Manager").await {
                Ok(connected) => {
                    println!("已连接设备 {}", connected.summary.id);
                    device = Some(connected);
                }
                Err(error) => eprintln!("回连失败：{error}"),
            }
        }
    }
}
