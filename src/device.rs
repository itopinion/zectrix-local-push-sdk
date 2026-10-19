use crate::{
    audio::{self, Recording},
    discovery, image,
    protocol::*,
    *,
};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
        Arc, Mutex as StdMutex,
    },
    time::Duration,
};
use tokio::{
    net::UdpSocket,
    sync::{mpsc, oneshot, watch, Mutex, Semaphore},
    time::{sleep, timeout, timeout_at, Instant},
};

type Pending = Arc<StdMutex<HashMap<(u8, u32), oneshot::Sender<Packet>>>>;
struct PendingGuard {
    key: (u8, u32),
    pending: Pending,
}
impl Drop for PendingGuard {
    fn drop(&mut self) {
        self.pending.lock().unwrap().remove(&self.key);
    }
}
struct Wire {
    socket: Arc<UdpSocket>,
    palette: DisplayPalette,
    id: u64,
    control: AtomicU32,
    query: AtomicU32,
    ping: AtomicU32,
    pending: Pending,
    command: Mutex<()>,
    shutdown: watch::Sender<Option<DisconnectReason>>,
    results: StdMutex<BTreeMap<(u8, u32), u8>>,
    last_ping: StdMutex<Instant>,
    image_gate: Arc<Semaphore>,
    image_upload: Mutex<()>,
    play_gate: Arc<Semaphore>,
    status_gate: Arc<Semaphore>,
    playback: AtomicU32,
    capture: AtomicU32,
    play_generation: AtomicU32,
    stopped_play: AtomicU32,
    stopped_capture: AtomicU32,
    slow: AtomicBool,
    recordings: AtomicUsize,
}
impl Wire {
    fn closed(&self) -> bool {
        self.shutdown.borrow().is_some()
    }
    fn seq(counter: &AtomicU32) -> Result<u32> {
        let mut n = counter.load(Ordering::SeqCst);
        loop {
            let next = n
                .checked_add(1)
                .filter(|n| *n < u32::MAX)
                .ok_or(Error::Disconnected)?;
            match counter.compare_exchange_weak(n, next, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => return Ok(n),
                Err(current) => n = current,
            }
        }
    }
    async fn send(&self, p: Packet) -> Result<()> {
        if self.closed() {
            return Err(Error::Disconnected);
        }
        self.socket.send(&p.encode()).await?;
        Ok(())
    }
    async fn request(&self, kind: u8, body: Vec<u8>, sequence: Option<u32>) -> Result<Packet> {
        let seq = match sequence {
            Some(n) => n,
            None => Self::seq(if kind == PING {
                &self.ping
            } else {
                &self.query
            })?,
        };
        let packet = Packet::new(kind, self.id, seq, body);
        let (tx, mut rx) = oneshot::channel();
        let key = (kind | 0x80, seq);
        {
            let mut pending = self.pending.lock().unwrap();
            if pending.len() >= 8 {
                return Err(Error::Busy);
            }
            pending.insert(key, tx);
        }
        let _guard = PendingGuard {
            key,
            pending: self.pending.clone(),
        };
        let mut shutdown = self.shutdown.subscribe();
        if self.closed() {
            return Err(Error::Disconnected);
        }
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut delay = Duration::from_millis(1500);
        loop {
            self.send(packet.clone()).await?;
            tokio::select! {biased;
                result=&mut rx=>return result.map_err(|_|Error::Disconnected).and_then(|p|{code(&p.body)?;Ok(p)}),
                _=shutdown.changed()=>return Err(Error::Disconnected),
                _=sleep(delay.min(deadline.saturating_duration_since(Instant::now())))=>{}
            }
            if Instant::now() >= deadline {
                return Err(Error::ResultUnknown);
            }
            delay = (delay * 2).min(Duration::from_secs(4));
        }
    }
    async fn control(&self, kind: u8, body: Vec<u8>) -> Result<Packet> {
        let _guard = if kind == AUDIO_STOP || kind == CLOSE {
            None
        } else {
            Some(self.command.lock().await)
        };
        let seq = Self::seq(&self.control)?;
        self.request(kind, body, Some(seq)).await
    }
    fn result(&self, object: u8, id: u32) -> Option<u8> {
        self.results.lock().unwrap().get(&(object, id)).copied()
    }
    fn store_result(&self, object: u8, id: u32, reason: u8) {
        let mut r = self.results.lock().unwrap();
        r.insert((object, id), reason);
        while r.len() > 16 {
            let key = *r.first_key_value().unwrap().0;
            r.remove(&key);
        }
    }
    async fn state_result(&self, object: u8, id: u32) -> Option<u8> {
        let p = timeout(Duration::from_secs(2), self.request(STATE, vec![], None))
            .await
            .ok()?
            .ok()?;
        if p.body.len() != 30 {
            return None;
        }
        let at = if object == 1 { 20 } else { 25 };
        if u32_at(&p.body, at) == Some(id) {
            Some(p.body[at + 4])
        } else {
            None
        }
    }
}
/// Connected device. Operations are safe to call from separate tasks.
/// Keep a single `next_event()` consumer and call `close()` before application exit.
pub struct Device {
    /// Identity returned by the device (the mDNS identity is verified against it).
    pub summary: DeviceSummary,
    wire: Arc<Wire>,
    events: Mutex<mpsc::Receiver<Event>>,
}
impl Drop for Device {
    fn drop(&mut self) {
        let _ = self
            .wire
            .shutdown
            .send_replace(Some(DisconnectReason::Closed));
    }
}
struct OpenGuard {
    socket: Arc<UdpSocket>,
    id: u64,
    armed: bool,
}
impl Drop for OpenGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = self
                .socket
                .try_send(&Packet::new(CLOSE, self.id, 2, vec![0]).encode());
        }
    }
}
impl Device {
    /// Discover one available device or open the supplied IPv4/ID target.
    /// Cancellation releases a connection that may already have been prepared.
    pub async fn open(options: OpenOptions) -> Result<Self> {
        let name = options
            .client_name
            .unwrap_or_else(|| "Zectrix Recorder".into());
        if name.is_empty() || name.len() > 48 || name.chars().any(char::is_control) {
            return Err(Error::InvalidOptions(
                "PC 名称应为 1～48 字节且不含控制字符".into(),
            ));
        }
        let targets = discovery::targets(&options.target).await?;
        let mut available = BTreeMap::new();
        let mut busy = false;
        for candidate in targets {
            let socket = Arc::new(UdpSocket::bind("0.0.0.0:0").await?);
            if socket.connect(&candidate.address).await.is_err() {
                continue;
            }
            let packet = Packet::new(INFO, 0, 1, vec![]);
            let response = match exchange(&socket, &packet, Duration::from_secs(2)).await {
                Ok(p) => p,
                Err(_) => continue,
            };
            let info = info(&response, candidate.address.clone())?;
            if !candidate.id.is_empty() && !info.id.eq_ignore_ascii_case(&candidate.id) {
                continue;
            }
            if info.busy {
                busy = true;
                continue;
            }
            available
                .entry(info.id.clone())
                .or_insert((info, socket, response.connection));
        }
        if available.is_empty() {
            return Err(if busy { Error::Busy } else { Error::NoDevice });
        }
        if available.len() > 1 {
            return Err(Error::MultipleDevices(
                available.into_values().map(|v| v.0).collect(),
            ));
        }
        let (summary, socket, id) = available.into_values().next().unwrap();
        let mut guard = OpenGuard {
            socket: socket.clone(),
            id,
            armed: true,
        };
        let mut body = vec![name.len() as u8];
        body.extend(name.as_bytes());
        let response = exchange(
            &socket,
            &Packet::new(CONNECT, id, 1, body),
            Duration::from_secs(15),
        )
        .await?;
        code(&response.body)?;
        // Query once before returning a usable device. OpenGuard releases ownership
        // on invalid/missing palettes or cancellation, without starting background work.
        let response = exchange(
            &socket,
            &Packet::new(PALETTE, id, 1, vec![]),
            Duration::from_secs(5),
        )
        .await
        .map_err(|e| match e {
            Error::ResultUnknown => {
                Error::TransportFailure("色板查询超时，请配套升级设备固件与 SDK".into())
            }
            other => other,
        })?;
        let palette = crate::protocol::palette(&response)?;
        let (shutdown, _) = watch::channel(None);
        let (tx, rx) = mpsc::channel(16);
        let wire = Arc::new(Wire {
            socket,
            palette,
            id,
            control: AtomicU32::new(2),
            query: AtomicU32::new(1),
            ping: AtomicU32::new(1),
            pending: Default::default(),
            command: Mutex::new(()),
            shutdown,
            results: Default::default(),
            last_ping: StdMutex::new(Instant::now()),
            image_gate: Arc::new(Semaphore::new(2)),
            image_upload: Mutex::new(()),
            play_gate: Arc::new(Semaphore::new(1)),
            status_gate: Arc::new(Semaphore::new(1)),
            playback: AtomicU32::new(0),
            capture: AtomicU32::new(0),
            play_generation: AtomicU32::new(0),
            stopped_play: AtomicU32::new(0),
            stopped_capture: AtomicU32::new(0),
            slow: AtomicBool::new(false),
            recordings: AtomicUsize::new(0),
        });
        tokio::spawn(receive(wire.clone(), tx));
        tokio::spawn(heartbeat(wire.clone()));
        guard.armed = false;
        Ok(Self {
            summary,
            wire,
            events: Mutex::new(rx),
        })
    }
    /// Palette read at connection establishment. This accessor sends no network request.
    pub fn palette(&self) -> &DisplayPalette {
        &self.wire.palette
    }
    /// Query battery percentage, charging and Wi-Fi RSSI without changing device
    /// state. No background polling; allow at least 30 seconds between UI polls.
    /// Battery is the device's last sampled estimate, including during standby.
    /// A concurrent status query returns Busy. Unsupported firmware times out.
    pub async fn status(&self) -> Result<DeviceStatus> {
        if self.wire.closed() {
            return Err(Error::Disconnected);
        }
        let _permit = self
            .wire
            .status_gate
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        let response = timeout(
            Duration::from_secs(5),
            self.wire.request(STATUS, vec![], None),
        )
        .await
        .map_err(|_| {
            Error::TransportFailure("状态查询超时：设备未响应或固件不支持此接口".into())
        })??;
        if self.wire.closed() {
            return Err(Error::Disconnected);
        }
        crate::protocol::status(&response)
    }

    /// Display a PNG in the full 400×300 canvas. Success means physical completion.
    /// Dropping this future does not cancel the submitted image.
    pub async fn show(&self, png: &[u8]) -> Result<()> {
        self.show_with_format(png, ImageFormat::Mono).await
    }
    /// Quantize/dither a PNG to black/white, four or sixteen shades on the host.
    /// The device automatically selects refresh and ghost-removal waveforms.
    pub async fn show_with_format(&self, png: &[u8], format: ImageFormat) -> Result<()> {
        let permit = self
            .wire
            .image_gate
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        if self.wire.capture.load(Ordering::SeqCst) != 0 {
            return Err(Error::Busy);
        }
        if png.len() > 8 * 1024 * 1024 {
            return Err(Error::InvalidImage("PNG 超过 8 MiB".into()));
        }
        let input = png.to_vec();
        let wire = self.wire.clone();
        let palette = wire.palette;
        tokio::spawn(async move {
            let _permit = permit;
            let data =
                tokio::task::spawn_blocking(move || image::prepare(&input, format, &palette))
                    .await
                    .map_err(|e| Error::InvalidImage(e.to_string()))??;
            show(wire, data, format).await
        })
        .await
        .map_err(|e| Error::TransportFailure(e.to_string()))?
    }
    /// Play fixed-format PCM; returns when playback ends, is stopped or interrupted.
    /// Use `stop()` from another task to cancel. Volume is temporary, 0 through 100.
    pub async fn play(&self, audio: &AudioClip, volume: u8) -> Result<PlaybackEnd> {
        if volume > 100 {
            return Err(Error::InvalidOptions("音量应为 0～100".into()));
        }
        if audio.samples.is_empty() {
            return Err(Error::InvalidAudio("没有样本".into()));
        }
        if audio.samples.len() > audio::MAX_SAMPLES {
            return Err(Error::AudioTooLong);
        }
        let permit = self
            .wire
            .play_gate
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        if self.wire.capture.load(Ordering::SeqCst) != 0 {
            return Err(Error::Busy);
        }
        let clip = audio.clone();
        let wire = self.wire.clone();
        let ticket = wire.play_generation.fetch_add(1, Ordering::SeqCst) + 1;
        tokio::spawn(async move {
            let _permit = permit;
            let n = clip.samples.len();
            let frames = tokio::task::spawn_blocking(move || audio::encode(&clip))
                .await
                .map_err(|e| Error::InvalidAudio(e.to_string()))??;
            if wire.play_generation.load(Ordering::SeqCst) != ticket {
                return Ok(PlaybackEnd::Stopped);
            }
            let result = play(
                wire.clone(),
                ticket,
                frames,
                ((n - 1) % 960 + 1) as u16,
                volume,
            )
            .await;
            wire.playback.store(0, Ordering::SeqCst);
            result
        })
        .await
        .map_err(|e| Error::TransportFailure(e.to_string()))?
    }
    /// Stop the active device playback or capture; idle is a successful no-op.
    pub async fn stop(&self) -> Result<()> {
        if self.wire.closed() {
            return Err(Error::Disconnected);
        }
        self.wire.play_generation.fetch_add(1, Ordering::SeqCst);
        let capture = self.wire.capture.load(Ordering::SeqCst);
        let play = self.wire.playback.load(Ordering::SeqCst);
        let id = if capture != 0 { capture } else { play };
        if id == 0 {
            return Ok(());
        }
        if capture != 0 {
            self.wire.stopped_capture.store(id, Ordering::SeqCst);
        } else {
            self.wire.stopped_play.store(id, Ordering::SeqCst);
        }
        match self
            .wire
            .control(AUDIO_STOP, id.to_be_bytes().to_vec())
            .await
        {
            Ok(_) => Ok(()),
            Err(Error::ResultUnknown) => {
                let p = self.wire.request(STATE, vec![], None).await?;
                if p.body.len() == 30 && p.body[3] == 0 {
                    Ok(())
                } else {
                    Err(Error::ResultUnknown)
                }
            }
            Err(e) => Err(e),
        }
    }
    /// Receive the next event. One consumer only; terminal connections return None.
    pub async fn next_event(&self) -> Result<Option<Event>> {
        let mut rx = self.events.lock().await;
        if self.wire.slow.swap(false, Ordering::SeqCst) {
            rx.close();
            while rx.try_recv().is_ok() {}
            return Err(Error::ConsumerTooSlow);
        }
        let event = rx.recv().await;
        if matches!(event, Some(Event::RecordingFinished { audio: Some(_), .. })) {
            self.wire.recordings.fetch_sub(1, Ordering::SeqCst);
        }
        Ok(event)
    }
    /// Release ownership and terminate local work. Safe to call repeatedly.
    pub async fn close(&self) -> Result<()> {
        if !self.wire.closed() {
            let _ = timeout(Duration::from_secs(2), self.wire.control(CLOSE, vec![0])).await;
            let _ = self
                .wire
                .shutdown
                .send_replace(Some(DisconnectReason::Closed));
        }
        Ok(())
    }
}
async fn exchange(socket: &UdpSocket, p: &Packet, duration: Duration) -> Result<Packet> {
    let deadline = Instant::now() + duration;
    let mut b = [0u8; 1201];
    loop {
        socket.send(&p.encode()).await?;
        let next = (Instant::now() + Duration::from_millis(1500)).min(deadline);
        while let Ok(n) = timeout_at(next, socket.recv(&mut b)).await {
            let n = n?;
            if let Some(r) = Packet::decode(&b[..n]) {
                if r.kind == p.kind | 0x80
                    && r.sequence == p.sequence
                    && (p.kind == INFO || r.connection == p.connection)
                {
                    return Ok(r);
                }
            }
        }
        if Instant::now() >= deadline {
            return Err(Error::ResultUnknown);
        }
    }
}
async fn show(wire: Arc<Wire>, data: Vec<u8>, format: ImageFormat) -> Result<()> {
    let mut upload = Some(wire.image_upload.lock().await);
    let mut begin = vec![format as u8];
    begin.extend((data.len() as u16).to_be_bytes());
    let id = wire.control(IMAGE_BEGIN, begin).await?.sequence;
    let chunks = data.chunks(1024).collect::<Vec<_>>();
    let all = u64::MAX >> (64 - chunks.len());
    let mut received = 0;
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut refreshing = false;
    let mut refresh_deadline = deadline;
    loop {
        if let Some(r) = wire.result(1, id) {
            return if r == 0 {
                Ok(())
            } else {
                Err(Error::DeviceFailure)
            };
        }
        if !refreshing {
            for (i, chunk) in chunks.iter().enumerate() {
                if received & (1 << i) == 0 {
                    let mut b = id.to_be_bytes().to_vec();
                    b.extend_from_slice(chunk);
                    wire.send(Packet::new(IMAGE_DATA, wire.id, i as u32, b))
                        .await?;
                    sleep(Duration::from_millis(2)).await;
                }
            }
        }
        let response = timeout(
            Duration::from_secs(2),
            wire.request(IMAGE_STATUS, id.to_be_bytes().to_vec(), None),
        )
        .await;
        if let Ok(Ok(p)) = response {
            if p.body.len() == 10 {
                received |= u64::from_be_bytes(p.body[2..10].try_into().unwrap()) & all;
                match p.body[1] {
                    3 => return Ok(()),
                    4 => return Err(Error::DeviceFailure),
                    2 if !refreshing => {
                        upload.take();
                        refreshing = true;
                        refresh_deadline = Instant::now() + Duration::from_secs(60);
                    }
                    _ => {}
                }
            }
        }
        if wire.closed() {
            return Err(Error::Disconnected);
        }
        if Instant::now()
            >= if refreshing {
                refresh_deadline
            } else {
                deadline
            }
        {
            return match wire.state_result(1, id).await {
                Some(0) => Ok(()),
                Some(_) => Err(Error::DeviceFailure),
                None => Err(Error::ResultUnknown),
            };
        }
        sleep(Duration::from_millis(350)).await;
    }
}
fn playback_result(wire: &Wire, id: u32, reason: u8) -> Result<PlaybackEnd> {
    if wire.stopped_play.load(Ordering::SeqCst) == id {
        return Ok(PlaybackEnd::Stopped);
    }
    match reason {
        0 => Ok(PlaybackEnd::Completed),
        5 => Ok(PlaybackEnd::Interrupted),
        6 => Ok(PlaybackEnd::Limit),
        _ => Err(Error::DeviceFailure),
    }
}
async fn play(
    wire: Arc<Wire>,
    ticket: u32,
    frames: Vec<Vec<u8>>,
    tail: u16,
    volume: u8,
) -> Result<PlaybackEnd> {
    let id = {
        let _command = wire.command.lock().await;
        if wire.play_generation.load(Ordering::SeqCst) != ticket {
            return Ok(PlaybackEnd::Stopped);
        }
        let id = Wire::seq(&wire.control)?;
        wire.playback.store(id, Ordering::SeqCst);
        let ack = wire.request(AUDIO_BEGIN, vec![volume], Some(id)).await;
        if wire.play_generation.load(Ordering::SeqCst) != ticket {
            return Ok(PlaybackEnd::Stopped);
        }
        ack?;
        id
    };
    let start = Instant::now();
    let total = frames.len();
    let mut shutdown = wire.shutdown.subscribe();
    for (i, frame) in frames.into_iter().enumerate() {
        tokio::select! {_ = tokio::time::sleep_until(start+Duration::from_millis(i as u64*60))=>{},_=shutdown.changed()=>return Err(Error::Disconnected)}
        if let Some(r) = wire.result(2, id) {
            return playback_result(&wire, id, r);
        }
        if wire.stopped_play.load(Ordering::SeqCst) == id {
            return Ok(PlaybackEnd::Stopped);
        }
        if Instant::now().duration_since(start + Duration::from_millis(i as u64 * 60))
            > Duration::from_millis(360)
        {
            let _ = wire.control(AUDIO_STOP, id.to_be_bytes().to_vec()).await;
            return Err(Error::TransportFailure("音频发送调度停顿".into()));
        }
        let mut body = id.to_be_bytes().to_vec();
        body.extend(frame);
        wire.send(Packet::new(AUDIO_DATA, wire.id, i as u32, body))
            .await?;
    }
    let mut body = id.to_be_bytes().to_vec();
    body.extend((total as u32).to_be_bytes());
    body.extend(tail.to_be_bytes());
    body.push(0);
    if let Err(e) = wire.control(AUDIO_END, body).await {
        if let Some(r) = wire.result(2, id) {
            return playback_result(&wire, id, r);
        }
        return Err(e);
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(r) = wire.result(2, id) {
            return playback_result(&wire, id, r);
        }
        if let Some(r) = wire.state_result(2, id).await {
            return playback_result(&wire, id, r);
        }
        if wire.closed() {
            return Err(Error::Disconnected);
        }
        if Instant::now() >= deadline {
            return Err(Error::ResultUnknown);
        }
        sleep(Duration::from_millis(500)).await;
    }
}
async fn heartbeat(wire: Arc<Wire>) {
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut shutdown = wire.shutdown.subscribe();
    loop {
        tokio::select! {_=shutdown.changed()=>break,_=interval.tick()=>{
            if wire.closed(){break;}
            if let Ok(Ok(_))=timeout(Duration::from_millis(4500),wire.request(PING,vec![],None)).await{*wire.last_ping.lock().unwrap()=Instant::now();}
        }}
    }
}
fn emit(wire: &Wire, tx: &mpsc::Sender<Event>, event: Event) -> bool {
    if matches!(&event, Event::RecordingFinished { audio: Some(_), .. })
        && wire.recordings.fetch_add(1, Ordering::SeqCst) >= 2
    {
        wire.slow.store(true, Ordering::SeqCst);
        return false;
    }
    if tx.try_send(event).is_err() {
        wire.slow.store(true, Ordering::SeqCst);
        return false;
    }
    true
}
fn finish_recording(
    wire: &Wire,
    tx: &mpsc::Sender<Event>,
    r: Recording,
    reason: RecordingEnd,
) -> bool {
    wire.capture.store(0, Ordering::SeqCst);
    let (audio, reason) = r.finish(reason);
    emit(wire, tx, Event::RecordingFinished { audio, reason })
}
fn rec_reason(n: u8) -> RecordingEnd {
    match n {
        0 => RecordingEnd::Completed,
        6 => RecordingEnd::Limit,
        _ => RecordingEnd::Failed,
    }
}
fn disconnect(n: u8) -> DisconnectReason {
    match n {
        0 => DisconnectReason::Closed,
        1 => DisconnectReason::DeviceExit,
        2 => DisconnectReason::Timeout,
        3 => DisconnectReason::Network,
        4 => DisconnectReason::System,
        _ => DisconnectReason::Failed,
    }
}
async fn receive(wire: Arc<Wire>, tx: mpsc::Sender<Event>) {
    let mut shutdown = wire.shutdown.subscribe();
    let mut buffer = [0u8; 1201];
    let mut tick = tokio::time::interval(Duration::from_millis(20));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut seen = VecDeque::<(u8, u32, Vec<u8>)>::new();
    let mut recording: Option<Recording> = None;
    let mut ended = 0;
    let reason = loop {
        tokio::select! {
            _=shutdown.changed()=>break shutdown.borrow().unwrap_or(DisconnectReason::Closed),
            _=tick.tick()=>{
                if wire.closed(){break wire.shutdown.borrow().unwrap_or(DisconnectReason::Closed);}
                if wire.last_ping.lock().unwrap().elapsed()>=Duration::from_secs(15){break DisconnectReason::Timeout;}
                let mut finish=None;
                if let Some(r)=recording.as_mut(){
                    r.stopped=wire.stopped_capture.load(Ordering::SeqCst)==r.id;
                    match r.ready(){Ok(true)=>finish=Some(RecordingEnd::Completed),Err(_)=>finish=Some(RecordingEnd::Failed),_=>{}}
                    if r.progress.elapsed()>Duration::from_secs(3)||r.started.elapsed()>Duration::from_secs(125){finish=Some(RecordingEnd::Failed);}
                }
                if let Some(why)=finish{let r=recording.take().unwrap();ended=ended.max(r.id);if !finish_recording(&wire,&tx,r,why){break DisconnectReason::Failed;}}
            },
            n=wire.socket.recv(&mut buffer)=>{
                let n=match n{Ok(n)=>n,Err(_)=>break DisconnectReason::Network};
                let Some(p)=Packet::decode(&buffer[..n]) else{continue};if p.connection!=wire.id{continue;}
                if p.kind&0x80!=0{
                    if let Some(sender)=wire.pending.lock().unwrap().remove(&(p.kind,p.sequence)){let _=sender.send(p.clone());}
                    if p.body.first()==Some(&3){break DisconnectReason::DeviceExit;}continue;
                }
                let valid=match p.kind{KEY=>p.body.len()==2 && (1..=3).contains(&p.body[0])&&(1..=2).contains(&p.body[1])&&!(p.body==[3,2]),RESULT=>p.body.len()==6&&(1..=2).contains(&p.body[4])&&p.body[5]<=7,CLOSE=>p.body.len()==1&&p.body[0]<=7,AUDIO_BEGIN=>p.body==[0]&&p.sequence>0,AUDIO_END=>p.body.len()==11&&p.body[10]<=7,AUDIO_DATA=>p.body.len()>=5&&p.body.len()<=1184,_=>false};
                if !valid{continue;}
                if p.kind!=AUDIO_DATA{
                    let _=wire.socket.send(&Packet::new(p.kind|0x80,wire.id,p.sequence,vec![0]).encode()).await;
                    if seen.iter().any(|(k,s,_)|*k==p.kind&&*s==p.sequence){continue;}
                    seen.push_back((p.kind,p.sequence,p.body.clone()));if seen.len()>256{seen.pop_front();}
                }
                match p.kind{
                    CLOSE=>break disconnect(p.body[0]),
                    KEY=>{let key=match p.body[0]{1=>Key::Up,2=>Key::Down,_=>Key::Confirm};let action=if p.body[1]==1{KeyAction::Short}else{KeyAction::Long};if !emit(&wire,&tx,Event::Key{key,action}){break DisconnectReason::Failed;}},
                    RESULT=>{let id=u32_at(&p.body,0).unwrap();wire.store_result(p.body[4],id,p.body[5]);},
                    AUDIO_BEGIN|AUDIO_DATA|AUDIO_END=>{
                        let id=if p.kind==AUDIO_BEGIN{p.sequence}else{u32_at(&p.body,0).unwrap()};
                        if id==0||id<=ended||recording.as_ref().is_some_and(|r|id<r.id){continue;}
                        if recording.as_ref().is_some_and(|r|r.id!=id){let r=recording.take().unwrap();ended=ended.max(r.id);if !finish_recording(&wire,&tx,r,RecordingEnd::Failed){break DisconnectReason::Failed;}}
                        if recording.is_none(){
                            recording=Recording::new(id).ok();if recording.is_none(){break DisconnectReason::Failed;}
                            wire.capture.store(id,Ordering::SeqCst);
                            let play=wire.playback.load(Ordering::SeqCst);if play!=0{wire.store_result(2,play,5);}
                            if p.kind!=AUDIO_END && !emit(&wire,&tx,Event::RecordingStarted){break DisconnectReason::Failed;}
                        }
                        let r=recording.as_mut().unwrap();r.stopped=wire.stopped_capture.load(Ordering::SeqCst)==id;
                        let result=match p.kind{
                            AUDIO_DATA=>r.push(p.sequence,&p.body[4..]),
                            AUDIO_END=>r.ending(u32_at(&p.body,4).unwrap(),u16::from_be_bytes([p.body[8],p.body[9]]),rec_reason(p.body[10])),_=>Ok(())
                        };
                        if result.is_err(){let r=recording.take().unwrap();ended=ended.max(r.id);if !finish_recording(&wire,&tx,r,RecordingEnd::Failed){break DisconnectReason::Failed;}}
                    },_=>{}
                }
            }
        }
    };
    if let Some(r) = recording {
        finish_recording(&wire, &tx, r, RecordingEnd::Disconnected);
    }
    let _ = wire.shutdown.send_replace(Some(reason));
    wire.pending.lock().unwrap().clear();
    let _ = tx.try_send(Event::Disconnected { reason });
    // Best effort release uses a new control sequence and the old connection ID.
    if let Ok(seq) = Wire::seq(&wire.control) {
        let _ = wire
            .socket
            .send(&Packet::new(CLOSE, wire.id, seq, vec![0]).encode())
            .await;
    }
}
