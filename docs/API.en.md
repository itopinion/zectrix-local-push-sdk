# Rust SDK API guide

[简体中文](API.md) | **English** | [Home](../README.en.md)

The SDK exposes PNG display, PCM audio and device events over ZECTRIX LAN v1. The entire 400×300 canvas belongs to the application. Discovery, chunking, retransmission, compression and Opus transport are handled internally. See [firmware setup](FIRMWARE.en.md) before connecting.

## Public methods

| Method | Meaning |
| --- | --- |
| `Device::open(OpenOptions).await` | Establish one device connection; failure does not start automatic reconnection |
| `device.palette()` | Read the palette cached during this connection, without a network request |
| `device.show(&png).await` | Display in monochrome and wait for device-reported completion |
| `device.show_with_format(&png, format).await` | Select `ImageFormat::Mono`, `Gray4` or `Gray16` |
| `device.play(&clip, volume).await` | Play PCM at volume 0–100; wait for `PlaybackEnd` |
| `device.stop().await` | Stop recording or playback; succeeds when already idle |
| `device.status().await` | Query battery, charging and Wi-Fi RSSI |
| `device.next_event().await` | Receive the next event; use a single event consumer |
| `device.close().await` | Best-effort release and local cleanup, waiting up to about two seconds; repeat-safe |

## Connection and ownership

```rust
use zectrix_sdk::{Device, OpenOptions};
let device = Device::open(OpenOptions {
    target: Some("192.168.1.100".into()),
    client_name: Some("My App".into()),
}).await?;
```

The target may be an IPv4 address, IPv4:port, a 12-character device ID, or omitted. Omitted targets use discovery for approximately five seconds. Addresses belonging to the same device ID are merged. Multiple available devices return `Error::MultipleDevices`; the caller must choose a target. Another controller's device is not taken over.

`client_name` is 1–48 UTF-8 bytes without control characters; the default is `Zectrix Recorder`. `device.summary` contains the actual identity, address and model. Discovery and INFO do not claim management ownership; `open` does. Share one connection using `Arc<Device>` for concurrent operations. Close it normally before another app takes over. Cancelled or failed setup attempts release prepared ownership on a best-effort basis.

The SDK rejects devices with an incompatible reserved display region. A connection reads PALETTE after CONNECT. Missing or invalid palette data fails setup and releases ownership. No guessed fallback is used. Reopening reads a fresh palette.

## Images and grayscale

Only PNG is accepted: at most 8 MiB encoded and 4096×4096 decoded. Supply 400×300 for exact coordinates. Other sizes are fitted proportionally and centered on white; a legacy 400×268 image is treated as an ordinary smaller image.

The SDK converts sRGB to linear RGB, computes luminance using 0.2126R + 0.7152G + 0.0722B, composites transparency onto white in linear light, then quantizes and applies Floyd–Steinberg error diffusion against the device's actual palette. Do not pre-dither or quantize in the application. sRGB 128 is approximately linear luminance 55 on a 0–255 scale.

`DisplayPalette.gray4` and `.gray16` contain four and sixteen `u8` linear luminance values indexed by device gray code. Duplicate levels are allowed; equal-distance choices prefer the lowest code. Never hard-code a palette from another device or connection.

```rust
use zectrix_sdk::ImageFormat;
device.show_with_format(&png, ImageFormat::Gray4).await?;
device.show_with_format(&png, ImageFormat::Gray16).await?;
```

The wire format uses IMAGE_BEGIN = u8 bpp + u16 compressed length and a u64 receive bitmap. Black-and-white, four-level and sixteen-level images use 1, 2 and 4 bits per pixel respectively. Firmware controls refresh waveforms and ghost removal; the SDK does not expose waveform selection.

At most two image operations can be in flight. Uploads are serialized; the device may receive the next frame while refreshing the current one. A full queue or recording can produce `Busy`. High-frequency producers should coalesce pending frames themselves. Each successful call waits for its own reported display completion. Dropping a submitted show future does not cancel the background operation. An unconfirmed outcome returns `ResultUnknown`, which must not trigger an automatic new image request.

Firmware notifications or recording indicators may temporarily overlay the remote image. A successful display acknowledgement does not prove the screen is unobscured or visually correct. The firmware retains the latest remote image beneath overlays.

## Telemetry

`DeviceStatus` contains `battery_percent: Option<u8>`, `charging: Option<bool>` and `wifi_rssi_dbm: Option<i8>`. Unknown values serialize to null, not zero or false. Battery is the latest voltage-based estimate; queries do not force a new ADC sample. Charging false does not necessarily mean USB is unplugged. RSSI is received signal strength in dBm, not throughput.

Telemetry works during media operations and does not use the event queue. Additional concurrent status queries return `Busy`. The SDK neither polls nor caches status responses. Applications should normally query no more often than every 30 seconds. Queries time out after roughly five seconds with same-sequence retries. A timeout is not proof of old firmware; it can also indicate connectivity problems. Query failure does not itself end a working connection, and cancellation frees the local waiter.

## Audio

`AudioClip { samples: Vec<i16> }` contains 16 kHz mono signed PCM16. When reading a PCM file, decode samples as little-endian. The SDK does not resample or decode arbitrary audio file formats.

```rust
let clip = zectrix_sdk::AudioClip { samples };
let end = device.play(&clip, 90).await?;
```

Empty audio returns `InvalidAudio`. More than 1,920,000 samples returns `AudioTooLong`. The protocol limit is 120 seconds, measured from preparation; clips close to the limit may end as `Limit`.

`PlaybackEnd` is `Completed`, `Stopped`, `Interrupted` or `Limit`. Recording gestures and audible firmware reminders can interrupt playback. Only one playback is active per connection; additional requests return `Busy`, without an automatic queue. Encoding runs in a background worker and does not block event reception. Dropping the play future does not stop playback; call `stop()` explicitly.

## Events and recording

```rust
use zectrix_sdk::Event;
while let Some(event) = device.next_event().await? {
    match event {
        Event::Key { key, action } => { /* Application navigation */ }
        Event::RecordingStarted => { /* Audio has actually started */ }
        Event::RecordingFinished { audio, reason } => { /* Save or process PCM */ }
        Event::Disconnected { reason } => { break; }
    }
}
```

Keys are `Up`, `Down`, `Confirm`; actions are `Short`, `Long`. Firmware sends Confirm short presses and Up/Down short or long presses. Confirm hold and the combined exit gesture are handled by recording/disconnection behavior. Use short-press filtering when assigning normal page navigation.

There is no remote-start recording API. Holding Confirm triggers recording on the device. The firmware displays its recording banner and plays the prompt before microphone capture; releasing during preparation may cancel it. `RecordingStarted` describes actual incoming audio, not merely the button gesture. The SDK can infer start from early audio data, deduplicates termination, reorders packets, conceals losses and trims frame padding. Recordings are processed voice audio, not raw ADC samples.

`RecordingFinished` delivers at most one full or partial clip per recording; zero audio frames can yield `None`. Reasons are `Completed`, `Stopped`, `Limit`, `Disconnected`, `Failed`. Move slow file processing away from the event-consumer loop.

The event queue holds 16 events, with at most two unconsumed recordings. A slow consumer can end the connection with `ConsumerTooSlow`. Disconnection completes any recording first, then produces the terminal event; subsequent calls return `None`.

## Errors and cleanup

Business errors include `NoDevice`, `MultipleDevices`, `Busy`, `Disconnected`, `InvalidImage`, `InvalidAudio`, `AudioTooLong`, `InvalidOptions`, `DeviceFailure`, `ResultUnknown`, `ConsumerTooSlow` and `TransportFailure`.

`ResultUnknown` means an operation may already have happened: do not automatically issue it again with a new request. Applications decide whether and when to retry `Busy`. After `Disconnected`, explicitly open a new connection; the SDK does not automatically reclaim the device.

One task consumes events; other tasks may display, play, stop or query using the same `Arc<Device>`. Stop and close requests have priority. On normal exit, await `close()`. Drop only initiates background cleanup; if the runtime stops immediately or the process crashes, device-side heartbeat expiry is the final release mechanism (about 15 seconds).

## Further reference

Generate the full Rust API reference with `cargo doc --locked --no-deps`. The [Chinese API document](API.md) and [binary protocol HTML](zectrix-lan-v1.html) contain further firmware interaction details. This guide describes the SDK 0.1.0 snapshot paired with firmware 3.7.2.
