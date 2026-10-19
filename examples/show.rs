//! cargo run --example show -- 192.168.1.100 card.png
use zectrix_sdk::{Device, ImageFormat, OpenOptions};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if !(3..=4).contains(&args.len()) {
        return Err("用法：show <IP 或设备 ID> <PNG 文件> [mono|gray4|gray16]".into());
    }
    let format = match args.get(3).map(String::as_str).unwrap_or("mono") {
        "mono" => ImageFormat::Mono,
        "gray4" => ImageFormat::Gray4,
        "gray16" => ImageFormat::Gray16,
        _ => return Err("图片格式只能为 mono、gray4 或 gray16".into()),
    };
    // Read before connecting so a file error cannot leave the device waiting for a lease timeout.
    let png = std::fs::read(&args[2])?;
    let device = Device::open(OpenOptions {
        target: Some(args[1].clone()),
        client_name: Some("Rust image example".into()),
    })
    .await?;
    let result = device.show_with_format(&png, format).await;
    device.close().await?;
    result?;
    println!("设备已完成显示");
    Ok(())
}
