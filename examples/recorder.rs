//! Capture one recording without Tauri or MP3 dependencies; save standard WAV.
use zectrix_sdk::{Device, Event, OpenOptions};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let device = Device::open(OpenOptions {
        target: std::env::args().nth(1),
        client_name: Some("Rust recorder example".into()),
    })
    .await?;
    println!("长按设备确认键录音，松开后保存 recording.wav");
    while let Some(event) = device.next_event().await? {
        match event {
            Event::RecordingStarted => println!("正在录音"),
            Event::RecordingFinished {
                audio: Some(audio),
                reason,
            } => {
                let n = (audio.samples.len() * 2) as u32;
                let mut b = Vec::new();
                b.extend(b"RIFF");
                b.extend((36 + n).to_le_bytes());
                b.extend(b"WAVEfmt ");
                b.extend(16u32.to_le_bytes());
                b.extend(1u16.to_le_bytes());
                b.extend(1u16.to_le_bytes());
                b.extend(16000u32.to_le_bytes());
                b.extend(32000u32.to_le_bytes());
                b.extend(2u16.to_le_bytes());
                b.extend(16u16.to_le_bytes());
                b.extend(b"data");
                b.extend(n.to_le_bytes());
                for x in audio.samples {
                    b.extend(x.to_le_bytes());
                }
                std::fs::write("recording.wav", b)?;
                println!("已保存，结束原因：{reason:?}");
                break;
            }
            Event::Disconnected { reason } => {
                println!("已断开：{reason:?}");
                break;
            }
            _ => {}
        }
    }
    device.close().await?;
    Ok(())
}
