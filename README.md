# ZECTRIX 本地推送 SDK

**简体中文** | [English](README.en.md)

让你的应用或 AI Agent 通过局域网直接连接 ZECTRIX Note 4：推送黑白、四灰阶和十六灰阶图片，播放音频，接收按键及录音。

本仓库提供 Rust SDK 源码、五个可编译运行的示例和协议文档，不包含固件二进制、烧录工具或桌面安装程序。

SDK 版本为 **0.1.0**，来源为 **2026-10-16 / 固件 3.7.2** 交付包。Rust crate 名仍为 `zectrix-sdk`，避免现有调用代码改名。此版本尚未发布到 crates.io。

## 设备准备与固件升级

支持设备：**ZECTRIX Note 4（zectrix-s3-epaper-4.2），400×300**。本版本配套固件为 **3.7.2**；不自动承诺其他固件版本兼容，也不适用于 Note 4C。

使用 Chrome 或 Edge 登录 [极趣云](https://cloud.zectrix.com/)，进入“本地升级”，用 USB 数据线连接 Note 4，选择匹配版本并按官方说明刷写。详见 [固件升级（中文）](docs/FIRMWARE.md) / [Firmware update (English)](docs/FIRMWARE.en.md) 和 [官方固件升级说明](https://wiki.zectrix.com/zh/software/firmware)。

升级后，在设备的“设置 → 网络同步”中启用“待机保持 Wi-Fi”功能。

## 快速开始

需要 Rust stable、平台 C/C++ 编译器和 CMake。SDK 使用原生 Opus。macOS 使用 Xcode Command Line Tools；Windows 使用 Rust MSVC 与 Visual Studio C++ Build Tools；Linux 可安装 build-essential、cmake、pkg-config。本次仓库整理在 Apple Silicon macOS 验证，其他平台需自行构建验证。首次构建需要联网下载 Cargo.lock 锁定的依赖。

```sh
git clone https://github.com/itopinion/zectrix-local-push-sdk.git
cd zectrix-local-push-sdk
cargo test --locked
cargo check --locked --examples
cargo run --locked --example status -- 192.168.1.100
cargo run --locked --example show -- 192.168.1.100 card.png gray16
```

将示例 IP 替换为自己的设备地址；`card.png` 使用自己的 PNG，推荐 400×300。图片格式可选 `mono`、`gray4`、`gray16`，省略时为黑白。如果本机 CMake 对旧版 Opus 构建脚本报兼容性错误，可设置环境变量 `CMAKE_POLICY_VERSION_MINIMUM=3.5` 后重试。

本仓库已公开，可直接克隆或下载源码。

## 在自己的应用中使用

将本仓库放在应用目录旁边：

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

长期运行的应用还应持续消费 `next_event()`，实现按键翻页、录音接收等行为。参考 `examples/keys.rs` 与 `examples/recorder.rs`。

## 示例

| 示例 | 功能 |
| --- | --- |
| `show` | PNG 推送；黑白、四灰、十六灰 |
| `status` | 电量、充电状态与 Wi-Fi RSSI |
| `keys` | 接收设备按键、退出事件 |
| `recorder` | 接收设备按键触发的录音并保存 WAV |
| `play` | 播放示例 PCM 音频 |

## 交给 Agent 开发

可以把下面这段需求交给自己的 Agent：

> 使用本仓库的 zectrix-sdk 开发一个本地应用，直接连接我指定的 Note 4。先读 README 和 docs/API.md，用当前目录旁的 SDK 路径依赖构建。生成 400×300 PNG，并按我的要求选择 mono、gray4 或 gray16。保持一个设备连接，持续接收按键事件，由应用管理页面；退出时调用 close。不要自动升级固件，不要连接未指定的其他设备。先通过单元测试和本机 UDP 测试，再根据设备显示完成回执报告推送结果。

## 运行约定

- 同一设备同时只由一个连接方管理。多个应用不要同时争用同一设备；正常退出调用 `close()`。
- 交互应用需要电脑上的程序持续运行；图片推送成功不等于应用已安装到设备。
- 提交原始连续色调 PNG；SDK 按设备实际色板转换并抖色，上层不要重复抖色。
- `show()` 成功表示设备报告显示完成，不仅是上传完成；视觉效果仍需实机观察，系统通知也可能临时覆盖画面。
- `ResultUnknown` 表示结果未知，不得自动作为新请求重新刷图或播放。`Busy` 由业务决定是否重试。
- 不自动重新接管断开的设备。无远程启动录音接口；录音由设备按键触发。
- 音频为 16 kHz、单声道 PCM16。SDK 不包含语音识别、TTS 或大模型。

## 文档与验证

- [中文 API](docs/API.md) / [English API guide](docs/API.en.md)
- [LAN 二进制协议（中文 HTML）](docs/zectrix-lan-v1.html)
- `cargo doc --locked --no-deps --open`：生成带英文源码注释的 Rust API 文档
- [兼容性清单](compatibility.json) / [本次验证记录](VALIDATION.json)
- `python3 scripts/verify.py`：核对本仓库文件清单与 SHA-256（Python 3 仅用于校验，不是 SDK 运行依赖）

源码、示例与测试保留上游实现；固件、烧录工具、资源二进制、旧固件路径清单和生成式 rustdoc 文件均未打包。修改源码后需由维护者重新生成 SDK_MANIFEST.json 和 SHA256SUMS。

## 许可

保留原始 [MIT License](LICENSE)；原生 Opus 许可见 [LICENSES/Opus-COPYING.txt](LICENSES/Opus-COPYING.txt)。Rust 依赖版本见 Cargo.lock，各组件遵循其上游许可。
