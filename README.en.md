# ZECTRIX Local Push SDK

[简体中文](README.md) | **English**

Connect your application or AI agent directly to a ZECTRIX Note 4 over your local network. Send monochrome, 4-level and 16-level grayscale images, play audio, and receive button and recording events.

This repository contains Rust SDK source, five buildable examples and protocol documentation. It does not distribute device firmware, flashing tools or a desktop installer.

SDK version: **0.1.0**, extracted from the **2026-10-16 / firmware 3.7.2** delivery. The Rust crate remains named `zectrix-sdk` for compatibility. It has not been published to crates.io.

## Device setup and firmware

Supported device: **ZECTRIX Note 4 (zectrix-s3-epaper-4.2), 400×300**. The matching firmware for this SDK snapshot is **3.7.2**. Compatibility with other versions is not assumed. Note 4C is not supported by this package.

Open [ZECTRIX Cloud](https://cloud.zectrix.com/) in Chrome or Edge, sign in, select **Local Upgrade**, connect Note 4 with a USB data cable, and follow the official instructions for the matching firmware. See [Firmware update](docs/FIRMWARE.en.md), [中文升级说明](docs/FIRMWARE.md), and the [official firmware guide](https://wiki.zectrix.com/zh/software/firmware).

After updating, enable **Keep Wi-Fi on during standby** (待机保持 Wi-Fi) under **Settings → Network Sync** (设置 → 网络同步) on the device.

## Quick start

Install Rust stable, a platform C/C++ compiler and CMake. The SDK uses native Opus. On macOS, install Xcode Command Line Tools; on Windows, use Rust MSVC and Visual Studio C++ Build Tools; on Linux, prepare build-essential, cmake and pkg-config. This extraction was validated on Apple Silicon macOS; other platforms require their own build and device validation. The first build needs internet access to fetch dependencies pinned in Cargo.lock.

```sh
git clone https://github.com/itopinion/zectrix-local-push-sdk.git
cd zectrix-local-push-sdk
cargo test --locked
cargo check --locked --examples
cargo run --locked --example status -- 192.168.1.100
cargo run --locked --example show -- 192.168.1.100 card.png gray16
```

Replace the example IP with your device address and supply your own PNG, preferably 400×300. The format is `mono`, `gray4` or `gray16`; the default is monochrome. If CMake rejects compatibility settings in the bundled Opus build, set the environment variable `CMAKE_POLICY_VERSION_MINIMUM=3.5` and retry.

This repository is public. You can clone it or download the source directly.

## Use from your own application

Place the SDK checkout next to your application directory:

```toml
[dependencies]
zectrix-sdk = { path = "../zectrix-local-push-sdk" }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

```rust
use zectrix_sdk::{Device, ImageFormat, OpenOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let png = std::fs::read("card.png")?;
    let device = Device::open(OpenOptions {
        target: Some("192.168.1.100".into()),
        client_name: Some("My Local App".into()),
    }).await?;
    let result = device.show_with_format(&png, ImageFormat::Gray16).await;
    device.close().await?;
    result?;
    Ok(())
}
```

Long-running applications must also keep consuming `next_event()` to handle buttons and recordings. See `examples/keys.rs` and `examples/recorder.rs`.

## Examples

| Example | Purpose |
| --- | --- |
| `show` | Send PNG images as monochrome, 4-level or 16-level grayscale |
| `status` | Battery percentage, charging state and Wi-Fi RSSI |
| `keys` | Receive button and disconnect events |
| `recorder` | Receive a device-triggered recording and save a WAV |
| `play` | Play sample PCM audio |

## Build with your agent

Example prompt:

> Build a local application using this repository's zectrix-sdk to connect directly to my specified Note 4. Read README.en.md and docs/API.en.md first, and use a path dependency to the adjacent SDK checkout. Generate 400×300 PNGs and use mono, gray4 or gray16 as requested. Keep one device connection, continuously consume button events, manage pages in the application, and call close on exit. Do not update firmware automatically or connect to another device. Run unit and local UDP tests first, then distinguish the device's display-completion acknowledgement from visual validation.

## Runtime behavior

- One controller owns a device at a time. Do not run multiple applications against the same device concurrently. Call `close()` on normal exit.
- Interactive applications need their computer process to remain running. Sending a picture does not install an application on the device.
- Supply continuous-tone PNGs. The SDK converts and dithers using the device palette; do not pre-dither in the application.
- A successful `show()` means the device reported display completion, not merely upload completion. Inspect visual quality on the actual screen; system notifications may temporarily overlay it.
- `ResultUnknown` means the result cannot be confirmed. Do not automatically issue a new display/playback request. The application decides whether to retry `Busy`.
- The SDK does not automatically reclaim a disconnected device. Recording starts from a physical device gesture; there is no remote-start recording API.
- Audio uses 16 kHz mono PCM16. Speech recognition, TTS and AI models are not included.

## Documentation and verification

- [English API guide](docs/API.en.md) / [中文 API](docs/API.md)
- [Binary LAN protocol specification (Chinese HTML)](docs/zectrix-lan-v1.html)
- `cargo doc --locked --no-deps --open`: generate Rust API documentation with English source comments
- [Compatibility metadata](compatibility.json) / [Validation record](VALIDATION.json)
- `python3 scripts/verify.py`: verify the file inventory and SHA-256 hashes (Python 3 is only needed for this check)

Upstream source, examples and tests are preserved. Firmware, flashing tools, resource binaries, obsolete firmware-path inventories and generated rustdoc assets are excluded. Maintainers must regenerate SDK_MANIFEST.json and SHA256SUMS after source changes.

## License

The original [MIT License](LICENSE) is retained. See [LICENSES/Opus-COPYING.txt](LICENSES/Opus-COPYING.txt) for native Opus. Rust dependency versions are pinned in Cargo.lock and remain subject to their upstream licenses.
