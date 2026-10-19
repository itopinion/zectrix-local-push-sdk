//! Play a short synthesized tone, using only the public PCM API.
use zectrix_sdk::{AudioClip, Device, OpenOptions};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let device = Device::open(OpenOptions {
        target: std::env::args().nth(1),
        client_name: Some("Rust audio example".into()),
    })
    .await?;
    let samples = (0..16000)
        .map(|i| ((i as f32 * 440. * std::f32::consts::TAU / 16000.).sin() * 5000.) as i16)
        .collect();
    let result = device.play(&AudioClip { samples }, 50).await;
    device.close().await?;
    println!("播放结果：{:?}", result?);
    Ok(())
}
