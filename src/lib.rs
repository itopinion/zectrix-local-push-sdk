//! A small, asynchronous SDK for Zectrix LAN devices.
//!
//! Start with [`Device::open`]. Audio is always 16 kHz mono signed 16-bit PCM.
//! Events have one consumer; use `Arc<Device>` to issue commands concurrently.
//! A lost connection is terminal: call `open` explicitly to create a new session.
mod audio;
mod device;
mod discovery;
mod image;
mod protocol;
pub use device::Device;
/// Packed image depth. Refresh waveform and ghost removal are device-managed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum ImageFormat {
    #[default]
    Mono = 1,
    Gray4 = 2,
    Gray16 = 4,
}
/// Device gray-code luminance in normalized linear light: 0=black, 255=white.
/// Entries are indexed by gray code; duplicate luminances are allowed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayPalette {
    pub gray4: [u8; 4],
    pub gray16: [u8; 16],
}
use serde::{Deserialize, Serialize};

/// All SDK operations return business errors, never raw wire result codes.
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("没有发现可用设备")]
    NoDevice,
    #[error("发现多台设备，请选择一台")]
    MultipleDevices(Vec<DeviceSummary>),
    #[error("设备忙碌，请稍后重试")]
    Busy,
    #[error("连接已结束，请显式重新连接")]
    Disconnected,
    #[error("图像无效：{0}")]
    InvalidImage(String),
    #[error("音频无效：{0}")]
    InvalidAudio(String),
    #[error("音频不能超过 120 秒")]
    AudioTooLong,
    #[error("无法确认操作结果，请勿自动重复执行")]
    ResultUnknown,
    #[error("事件消费过慢，连接已结束")]
    ConsumerTooSlow,
    #[error("设备处理失败")]
    DeviceFailure,
    #[error("参数无效：{0}")]
    InvalidOptions(String),
    #[error("通信失败：{0}")]
    TransportFailure(String),
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::TransportFailure(e.to_string())
    }
}
/// `target`: None discovers one available device; otherwise an IPv4 address,
/// `IPv4:port`, or a stable device ID. No automatic reconnect is performed.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct OpenOptions {
    pub target: Option<String>,
    pub client_name: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceSummary {
    pub id: String,
    pub name: String,
    pub address: String,
    pub busy: bool,
}
/// Read-only device telemetry. `None` means the device cannot provide a value.
/// Battery is its latest ADC-based estimate; querying does not force sampling.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceStatus {
    pub battery_percent: Option<u8>,
    pub charging: Option<bool>,
    /// Received Wi-Fi signal level in dBm, not a percentage or link-speed test.
    pub wifi_rssi_dbm: Option<i8>,
}
/// Fixed-format PCM. Duration is `samples.len() / 16000` seconds.
#[derive(Clone, Debug, Default)]
pub struct AudioClip {
    pub samples: Vec<i16>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Key {
    Up,
    Down,
    Confirm,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyAction {
    Short,
    Long,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecordingEnd {
    Completed,
    Stopped,
    Limit,
    Disconnected,
    Failed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaybackEnd {
    Completed,
    Stopped,
    Interrupted,
    Limit,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisconnectReason {
    Closed,
    DeviceExit,
    Timeout,
    Network,
    System,
    Failed,
}
#[derive(Debug)]
pub enum Event {
    Key {
        key: Key,
        action: KeyAction,
    },
    RecordingStarted,
    RecordingFinished {
        audio: Option<AudioClip>,
        reason: RecordingEnd,
    },
    Disconnected {
        reason: DisconnectReason,
    },
}
