//! Wire-level tests use real loopback UDP and independent packet construction.
use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{net::UdpSocket, task::JoinHandle, time::timeout};
use zectrix_sdk::{AudioClip, Device, Event, Key, KeyAction, OpenOptions, PlaybackEnd};
fn packet(kind: u8, seq: u32, body: &[u8]) -> Vec<u8> {
    let mut b = vec![b'Z', b'L', 1, kind];
    b.extend(0x1122334455667788u64.to_be_bytes());
    b.extend(seq.to_be_bytes());
    b.extend(body);
    b
}
fn num(b: &[u8], i: usize) -> u32 {
    u32::from_be_bytes(b[i..i + 4].try_into().unwrap())
}
struct Peer {
    socket: Arc<UdpSocket>,
    client: Arc<Mutex<Option<SocketAddr>>>,
    task: JoinHandle<()>,
    begins: Arc<AtomicUsize>,
    pings: Arc<AtomicUsize>,
    closes: Arc<AtomicUsize>,
    status_reply: Arc<Mutex<Option<Vec<u8>>>>,
    status_queries: Arc<AtomicUsize>,
    palette_queries: Arc<AtomicUsize>,
    palette_reply: Arc<Mutex<Option<Vec<u8>>>>,
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Peer {
    async fn new() -> Self {
        let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let client = Arc::new(Mutex::new(None));
        let begins = Arc::new(AtomicUsize::new(0));
        let pings = Arc::new(AtomicUsize::new(0));
        let closes = Arc::new(AtomicUsize::new(0));
        let status_reply = Arc::new(Mutex::new(Some(vec![73, 0, 195])));
        let status_queries = Arc::new(AtomicUsize::new(0));
        let palette_queries = Arc::new(AtomicUsize::new(0));
        let mut palette_body = vec![0, 0, 85, 170, 255];
        palette_body.extend((0u8..16).map(|n| n * 17));
        let palette_reply = Arc::new(Mutex::new(Some(palette_body)));
        let palette_response = palette_reply.clone();
        let palette_queried = palette_queries.clone();
        let status_body = status_reply.clone();
        let queried = status_queries.clone();
        let closed = closes.clone();
        let (s, c, n, p) = (
            socket.clone(),
            client.clone(),
            begins.clone(),
            pings.clone(),
        );
        let task = tokio::spawn(async move {
            let mut b = [0u8; 1201];
            let mut image = 0;
            let mut bitmap = 0u64;
            let mut playback = 0;
            let mut dropped_begin = false;
            loop {
                let (len, from) = s.recv_from(&mut b).await.unwrap();
                if len < 16 || b[3] & 128 != 0 {
                    continue;
                }
                let kind = b[3];
                let seq = num(&b, 12);
                let mut response = vec![0];
                match kind {
                    16 => {
                        assert_eq!(len, 16);
                        palette_queried.fetch_add(1, Ordering::SeqCst);
                        let body = palette_response.lock().unwrap().clone();
                        match body {
                            Some(body) => response = body,
                            None => continue,
                        }
                    }
                    15 => {
                        assert_eq!(len, 16);
                        queried.fetch_add(1, Ordering::SeqCst);
                        let reply = status_body.lock().unwrap().clone();
                        match reply {
                            Some(bytes) => response.extend(bytes),
                            None => continue,
                        }
                    }
                    1 => {
                        response.extend([0, 12]);
                        response.extend(b"aabbccddeeff");
                        response.push(4);
                        response.extend(b"test");
                        response.extend([1, 144, 1, 44, 0, 0, 0, 0, 0, 0, 0, 0]);
                    }
                    2 => {
                        *c.lock().unwrap() = Some(from);
                    }
                    3 => {
                        p.fetch_add(1, Ordering::SeqCst);
                    }
                    5 => {
                        if image != seq {
                            image = seq;
                            n.fetch_add(1, Ordering::SeqCst);
                        }
                        if !dropped_begin {
                            dropped_begin = true;
                            continue;
                        }
                    }
                    6 => {
                        bitmap |= 1 << seq;
                        continue;
                    }
                    7 => {
                        response.extend([3]);
                        response.extend(bitmap.to_be_bytes());
                    }
                    8 => {
                        playback = seq;
                    }
                    9 => continue,
                    10 | 11 => {
                        s.send_to(&packet(kind | 128, seq, &response), from)
                            .await
                            .unwrap();
                        let mut done = playback.to_be_bytes().to_vec();
                        done.extend([2, 0]);
                        s.send_to(&packet(13, 100 + seq, &done), from)
                            .await
                            .unwrap();
                        playback = 0;
                        continue;
                    }
                    12 => {
                        response.extend([2, 3, if playback == 0 { 0 } else { 2 }]);
                        response.extend(image.to_be_bytes());
                        response.extend(bitmap.to_be_bytes());
                        response.extend(playback.to_be_bytes());
                        response.extend(image.to_be_bytes());
                        response.push(0);
                        response.extend(0u32.to_be_bytes());
                        response.push(0);
                    }
                    4 => {
                        closed.fetch_add(1, Ordering::SeqCst);
                    }
                    _ => continue,
                }
                s.send_to(&packet(kind | 128, seq, &response), from)
                    .await
                    .unwrap();
            }
        });
        Self {
            socket,
            client,
            task,
            begins,
            pings,
            closes,
            status_reply,
            status_queries,
            palette_queries,
            palette_reply,
        }
    }
    async fn open(&self) -> Device {
        Device::open(OpenOptions {
            target: Some(self.socket.local_addr().unwrap().to_string()),
            client_name: Some("test".into()),
        })
        .await
        .unwrap()
    }
    async fn event(&self, kind: u8, seq: u32, body: &[u8]) {
        let addr = self.client.lock().unwrap().unwrap();
        self.socket
            .send_to(&packet(kind, seq, body), addr)
            .await
            .unwrap();
    }
}
#[tokio::test]
async fn status_is_on_demand_unknown_safe_and_does_not_break_media() {
    let p = Peer::new().await;
    let d = p.open().await;
    assert_eq!(p.status_queries.load(Ordering::SeqCst), 0);
    let status = d.status().await.unwrap();
    assert_eq!(status.battery_percent, Some(73));
    assert_eq!(status.charging, Some(false));
    assert_eq!(status.wifi_rssi_dbm, Some(-61));
    *p.status_reply.lock().unwrap() = Some(vec![255, 255, 127]);
    assert_eq!(
        d.status().await.unwrap(),
        zectrix_sdk::DeviceStatus::default()
    );
    *p.status_reply.lock().unwrap() = Some(vec![101, 1, 195]);
    assert!(matches!(
        d.status().await,
        Err(zectrix_sdk::Error::TransportFailure(_))
    ));
    *p.status_reply.lock().unwrap() = Some(vec![0, 1, 195]);
    let clip = AudioClip {
        samples: vec![0; 960],
    };
    let (status, play) = tokio::join!(d.status(), d.play(&clip, 60));
    assert_eq!(status.unwrap().battery_percent, Some(0));
    assert_eq!(play.unwrap(), PlaybackEnd::Completed);
    d.close().await.unwrap();
    assert!(matches!(
        d.status().await,
        Err(zectrix_sdk::Error::Disconnected)
    ));
}

#[tokio::test]
async fn status_timeout_or_cancellation_leaves_the_connection_usable() {
    let p = Peer::new().await;
    let d = p.open().await;
    *p.status_reply.lock().unwrap() = None; // Old firmware ignores opcode 15.
    assert!(timeout(Duration::from_millis(100), d.status())
        .await
        .is_err());
    assert!(matches!(
        timeout(Duration::from_secs(6), d.status()).await.unwrap(),
        Err(zectrix_sdk::Error::TransportFailure(_))
    ));
    assert!(p.pings.load(Ordering::SeqCst) > 0);
    *p.status_reply.lock().unwrap() = Some(vec![51, 1, 180]);
    assert_eq!(d.status().await.unwrap().wifi_rssi_dbm, Some(-76));
    d.close().await.unwrap();
}
#[tokio::test]
async fn key_dedup_out_of_order_and_terminal_exit() {
    let p = Peer::new().await;
    let d = p.open().await;
    p.event(14, 11, &[1, 1]).await;
    p.event(14, 10, &[2, 2]).await;
    p.event(14, 11, &[1, 1]).await;
    assert!(matches!(
        d.next_event().await.unwrap(),
        Some(Event::Key {
            key: Key::Up,
            action: KeyAction::Short
        })
    ));
    assert!(matches!(
        d.next_event().await.unwrap(),
        Some(Event::Key {
            key: Key::Down,
            action: KeyAction::Long
        })
    ));
    p.event(4, 12, &[1]).await;
    assert!(matches!(
        d.next_event().await.unwrap(),
        Some(Event::Disconnected { .. })
    ));
    assert!(d.next_event().await.unwrap().is_none());
    assert!(d
        .play(
            &AudioClip {
                samples: vec![0; 960]
            },
            70
        )
        .await
        .is_err());
    d.close().await.unwrap();
}
#[tokio::test]
async fn images_retry_same_begin_and_preserve_heartbeat() {
    let p = Peer::new().await;
    let d = p.open().await;
    let im = image::RgbImage::from_pixel(400, 300, image::Rgb([0, 0, 0]));
    let mut data = std::io::Cursor::new(Vec::new());
    im.write_to(&mut data, image::ImageFormat::Png).unwrap();
    timeout(Duration::from_secs(5), d.show(data.get_ref()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(p.begins.load(Ordering::SeqCst), 1);
    assert!(p.pings.load(Ordering::SeqCst) > 0);
    d.close().await.unwrap();
}
#[tokio::test]
async fn playback_completion_and_concurrent_stop() {
    let p = Peer::new().await;
    let d = Arc::new(p.open().await);
    assert_eq!(
        d.play(
            &AudioClip {
                samples: vec![200; 961]
            },
            50
        )
        .await
        .unwrap(),
        PlaybackEnd::Completed
    );
    let dd = d.clone();
    let playing = tokio::spawn(async move {
        dd.play(
            &AudioClip {
                samples: vec![300; 16000 * 5],
            },
            70,
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    d.stop().await.unwrap();
    assert_eq!(playing.await.unwrap().unwrap(), PlaybackEnd::Stopped);
    d.close().await.unwrap();
}
#[tokio::test]
async fn audio_before_begin_is_assembled_once_with_exact_tail() {
    let p = Peer::new().await;
    let d = p.open().await;
    let mut encoder =
        opus::Encoder::new(16000, opus::Channels::Mono, opus::Application::Audio).unwrap();
    let frame = encoder.encode_vec(&vec![100; 960], 1180).unwrap();
    let mut body = 20u32.to_be_bytes().to_vec();
    body.extend(&frame);
    p.event(9, 1, &body).await;
    p.event(9, 0, &body).await;
    p.event(8, 20, &[0]).await;
    let mut end = 20u32.to_be_bytes().to_vec();
    end.extend(2u32.to_be_bytes());
    end.extend(123u16.to_be_bytes());
    end.push(0);
    p.event(10, 21, &end).await;
    p.event(10, 21, &end).await;
    assert!(matches!(
        d.next_event().await.unwrap(),
        Some(Event::RecordingStarted)
    ));
    let event = timeout(Duration::from_secs(2), d.next_event())
        .await
        .unwrap()
        .unwrap();
    if let Some(Event::RecordingFinished {
        audio: Some(audio), ..
    }) = event
    {
        assert_eq!(audio.samples.len(), 1083)
    } else {
        panic!("{event:?}")
    }
    p.event(4, 22, &[1]).await;
    assert!(matches!(
        d.next_event().await.unwrap(),
        Some(Event::Disconnected { .. })
    ));
    d.close().await.unwrap();
}
#[tokio::test]
async fn cancellation_during_connect_releases_prepared_owner() {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let target = socket.local_addr().unwrap().to_string();
    let mut task = tokio::spawn(Device::open(OpenOptions {
        target: Some(target),
        client_name: None,
    }));
    let mut b = [0u8; 1201];
    let (_, peer) = socket.recv_from(&mut b).await.unwrap();
    let mut info = vec![0, 0, 12];
    info.extend(b"aabbccddeeff");
    info.push(4);
    info.extend(b"test");
    info.extend([1, 144, 1, 44, 0, 0, 0, 0, 0, 0, 0, 0]);
    socket.send_to(&packet(129, 1, &info), peer).await.unwrap();
    socket.recv_from(&mut b).await.unwrap();
    assert_eq!(b[3], 2);
    task.abort();
    let _ = (&mut task).await;
    let (n, _) = timeout(Duration::from_secs(1), socket.recv_from(&mut b))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(b[3], 4);
    assert_eq!(n, 17);
}

#[tokio::test]
async fn immediate_drop_releases_even_before_workers_are_polled() {
    let peer = Peer::new().await;
    let device = peer.open().await;
    drop(device);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(peer.closes.load(Ordering::SeqCst) > 0);
}
#[tokio::test]
async fn stop_during_encoding_does_not_start_a_late_playback() {
    let peer = Peer::new().await;
    let device = Arc::new(peer.open().await);
    let audio = AudioClip {
        samples: vec![50; 16000 * 30],
    };
    let mut pending = Box::pin(device.play(&audio, 60));
    // Poll once so the public call has accepted the job before requesting stop.
    std::future::poll_fn(|cx| {
        assert!(std::future::Future::poll(pending.as_mut(), cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    device.stop().await.unwrap();
    assert_eq!(pending.await.unwrap(), PlaybackEnd::Stopped);
    device.close().await.unwrap();
}

#[tokio::test]
async fn palette_is_loaded_once_per_connection_and_errors_release_ownership() {
    let p = Peer::new().await;
    let d = p.open().await;
    assert_eq!(d.palette().gray4, [0, 85, 170, 255]);
    assert_eq!(d.palette().gray16[15], 255);
    assert_eq!(p.palette_queries.load(Ordering::SeqCst), 1);
    p.palette_reply.lock().unwrap().as_mut().unwrap()[2] = 45;
    assert_eq!(d.palette().gray4[1], 85); // Session cache is immutable.
    d.close().await.unwrap();
    let d = p.open().await;
    assert_eq!(d.palette().gray4[1], 45);
    assert_eq!(p.palette_queries.load(Ordering::SeqCst), 2);
    d.close().await.unwrap();

    // Malformed, unavailable and missing palettes must not silently become a host default.
    for reply in [Some(vec![0, 0, 255]), Some(vec![4]), None] {
        *p.palette_reply.lock().unwrap() = reply;
        let closes = p.closes.load(Ordering::SeqCst);
        let result = Device::open(OpenOptions {
            target: Some(p.socket.local_addr().unwrap().to_string()),
            client_name: Some("palette test".into()),
        })
        .await;
        assert!(result.is_err());
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(p.closes.load(Ordering::SeqCst) > closes);
    }
}
