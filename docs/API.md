# Rust SDK API

**简体中文** | [English](API.en.md) | [固件升级](FIRMWARE.md)

SDK 面向 Zectrix LAN v1，开放完整 400×300 墨水屏画布，没有固定保留区。开发者提供 PNG 和 PCM，接收按键与录音事件。所有网络、发现、Opus 和分块细节留在 SDK 内部。

Zectrix 固件的录音 PCM 会经过语音处理，采样格式仍为 16 kHz 单声道 PCM16。它适合语音应用；SDK 不提供未经处理的 ADC 数据。播放 PCM 不经过麦克风语音前处理。

## 在自己的项目中使用

首版尚未发布到 crates.io，可通过路径依赖引用源码：

```toml
[dependencies]
zectrix-sdk = { path = "../zectrix-local-push-sdk" }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

按实际目录修改路径，保留本独立包的 `Cargo.toml` 和 `Cargo.lock`。独立使用 SDK 不需要 Tauri、React 或桌面图形库。

## 主要接口

| 接口 | 结果和含义 |
| --- | --- |
| `Device::open(OpenOptions).await` | 返回连接完成的 `Device`；失败不会在后台自动重新连接 |
| `device.palette()` | 返回连接时读取的 `&DisplayPalette`，本地只读访问，不发网络请求 |
| `device.show(&png_bytes).await` | 默认黑白；返回 `Result<()>`，设备报告实际显示完成才成功 |
| `device.show_with_format(&png_bytes, ImageFormat::Gray4).await` | 指定 Mono / Gray4 / Gray16；刷新与清残影由设备管理 |
| `device.play(&AudioClip, volume).await` | 返回 `Result<PlaybackEnd>`；等待播放结束，音量为 0～100 |
| `device.stop().await` | 请求停止当前采集或播放；空闲为成功，无远程开启录音功能 |
| `device.status().await` | 查询电量百分比、是否充电、Wi-Fi RSSI；不改变设备状态 |
| `device.next_event().await` | 返回 `Result<Option<Event>>`；一个事件消费者，终止后返回 `None` |
| `device.close().await` | 尽力释放并清理本地会话，最多等待约 2 秒；重复调用安全 |

`Device.summary` 包含真实设备身份、地址和型号。需要并行处理事件、播放和停止时使用 `Arc<Device>`，不创建多个同设备连接。

## 打开设备

```rust
use zectrix_sdk::{Device, OpenOptions};
let device = Device::open(OpenOptions {
    target: Some("192.168.1.100".into()),
    client_name: Some("My Recorder".into()),
}).await?;
```

`target` 可以省略，或填写 IPv4、`IPv4:端口`、12 位设备 ID。默认端口由发现记录决定，直接 IP 使用 32342。`client_name` 为 1～48 字节的 UTF-8，不含控制字符；默认 `Zectrix Recorder`。

省略目标时默认发现 5 秒，验证设备身份和可用状态后连接唯一设备。多个地址下的同一设备按 ID 合并；多台可用设备返回 `Error::MultipleDevices(Vec<DeviceSummary>)`，调用者选择后再次调用 `open`。其他 PC 管理中的设备不会被抢占。指定 ID 会通过发现解析地址。

发现和 INFO 查询不取得管理权；`open` 才是明确的连接操作。连接准备失败、断网或本地退出后，该对象终止，不在后台自动取新连接编号重连。取消 `open` 会尽力释放可能已准备的连接。

## 设备状态

```rust
let status = device.status().await?;
// Option<u8>，0～100；None 表示不可用，Some(0) 是确实测得 0%。
let battery = status.battery_percent;
// Option<bool>；是否正在给电池充电，不等于“插着 USB”。
let charging = status.charging;
// Option<i8>，单位 dBm，例如 -61；不是百分比。
let wifi_rssi = status.wifi_rssi_dbm;
```

`DeviceStatus` 只含上述三个字段，实现 `Clone / Debug / Default / PartialEq / Eq / Serialize / Deserialize`。未知值序列化为 `null`，SDK 不把它们换算成 0 或 false。

- 电量是电压估算值，返回固件最近一次采样缓存。返回值不是每次查询时重新测量的结果；尚无有效样本或最近采样失败时返回 `None`。
- 充电返回设备已判定的充电状态；电池充满、充电被限制或没有外部供电，都可能返回 `Some(false)`。
- Wi-Fi 返回已关联接入点的 RSSI，不扫描周围热点。获取失败时返回 `None`；它不表示吞吐量或端到端延迟。
- 仅在已连接的 `Device` 上调用。播放、刷屏和录音期间也可查询，完成结果不从事件队列交付。已关闭或断开的对象返回 `Disconnected`。
- SDK 不主动轮询，也不缓存查询响应。应用状态栏建议至少间隔 30 秒按需查询；频繁请求仍会增加网络唤醒和功耗。同一设备同时进行多个状态查询时，额外调用返回 `Busy`。
- 查询等待上限约 5 秒，期间按相同序号重试。旧固件不支持这个新增请求时会超时，返回 `TransportFailure`，提示检查连接及固件版本；不能把超时断言为“固件版本一定过旧”。状态查询失败不会主动断开已经建立的连接；当前 SDK 的连接阶段仍要求固件支持 PALETTE。取消查询会释放本地等待资源。

运行独立示例：`cargo run -p zectrix-sdk --example status -- 192.168.1.100`。

## 图像

`show` 接受 PNG，最大编码大小 8 MiB、解码尺寸 4096×4096。推荐输入 400×300 完整画面；同尺寸图像按原坐标显示。其他尺寸按比例缩放到完整画布，白底居中；PNG 按 sRGB 解码到线性 RGB，以 0.2126R+0.7152G+0.0722B 计算亮度并在线性域合成白底。远端按设备色板的实际亮度进行量化及 Floyd–Steinberg 误差扩散，设备不做二次抖色。

SDK 自动合成 400×300 完整画布、压缩、补传及查询结果，不挪动或清空顶部像素。不接受 SVG、JPEG 或 JSON 绘图命令；应用可以用现成渲染库生成 PNG。当前录音时返回 `Busy`。最多两个图片任务并发：SDK 串行上传，设备刷新当前帧时可接收、校验并缓存下一帧；缓存已满时返回 `Busy`。每个调用仍等待各自实际显示完成。

首次连接时设备用自带字体显示默认远程页面，收到图片后由应用画面替换；全部 400×300 像素由应用控制，不显示固定标题栏或控制端名称。待办到期时，固件可临时在画面上覆盖通知，连接保持；有声提醒播放设备提示音，远程音频若正在播放会被中断并返回 Interrupted。通知约 12 秒后自动收起，也可用一次新的确认键按压收起并停声；该次手势不会产生 SDK 按键或录音事件，不会完成待办。通知出现前已经开始的录音仍可正常松键结束。固件单独保留完整远程原图，通知和录音提示只覆盖合成画面；期间发送的新图片会完整保留，收起后恢复最新图片的被覆盖部分；`show` 成功表示显示刷新完成，不保证此时没有系统通知覆盖。开发者无需新增调用。

`show` 的 future 被丢弃后，已提交的操作仍在后台收尾。断开连接使用 `close`。结果丢失时内部查询状态，最终仍无法确认则返回 `ResultUnknown`，不自动重新刷图。

`Device::open()` 在 CONNECT 成功后自动读取一次 PALETTE（type=16），成功响应固定 21 字节。`device.palette().gray4` / `.gray16` 分别是 4/16 个 `u8` 亮度值，下标是显示灰号，0/255 为屏幕黑白端点，数值在线性亮度域。相同距离或重复亮度选择最小灰号。连接期间色板固定，重新连接重新读取；读取失败或内容非法会使 open 失败，并释放管理权，不使用电脑端猜测值。

色板以每次连接查询到的数值为准，不应在应用中写死。SDK 自动用于最近亮度选择与误差扩散。色板数值在线性亮度域；sRGB 128 约对应线性亮度 55。

指定图片格式：

```rust
use zectrix_sdk::ImageFormat;
device.show_with_format(&png_bytes, ImageFormat::Gray4).await?;
device.show_with_format(&png_bytes, ImageFormat::Gray16).await?;
```

协议首版统一为 `IMAGE_BEGIN = u8 bpp + u16 压缩长度`，分片接收位图为 `u64`。固件与 SDK 必须配套更新。非法 bpp/长度返回 INVALID；坏 zlib、截断、尾随数据或解压长度不符返回图片失败，保持原画面。不要把发送完成当成显示完成。

## 音频

```rust
let clip = zectrix_sdk::AudioClip { samples: pcm_i16 };
let end = device.play(&clip, 70).await?;
```

`AudioClip.samples` 是 16 kHz、单声道、有符号 16 位 PCM；文件转换时按小端处理。没有编码器、网络帧或流 ID 参数。SDK 不承担任意文件格式解码和重采样。

空音频返回 `InvalidAudio`，超过 1,920,000 个样本返回 `AudioTooLong`。120 秒是协议上限；设备从准备开始计时，极接近上限的播放可能以 `Limit` 结束，不能保证无缝长音频续播。

`PlaybackEnd`：

- `Completed`：设备确认正常播完。
- `Stopped`：调用 `stop` 停止，包括正在编码准备的播放。
- `Interrupted`：被设备确认键长按录音或有声待办提醒打断。
- `Limit`：设备时长上限。

`play` 内部在后台工作任务编码，不阻塞收发循环。调用者可以在另一任务继续消费按键事件或调用 `stop`。同一连接只接受一个当前播放，额外播放返回 `Busy`；不自动排队。丢弃等待 future 不意味着停止，明确停止请调用 `stop`。

## 四种事件

```rust
match device.next_event().await? {
    Some(Event::Key { key, action }) => { /* 业务映射 */ }
    Some(Event::RecordingStarted) => { /* 显示录音状态 */ }
    Some(Event::RecordingFinished { audio, reason }) => { /* 保存 PCM */ }
    Some(Event::Disconnected { reason }) => { /* 显示重新连接入口 */ }
    None => { /* 当前对象结束 */ }
}
```

- `Key`：`Up / Down / Confirm`；`KeyAction`：`Short / Long`。固件实际产生确认短按、上下短按和上下长按。确认长按与组合退出由录音／断开流程消费。
- `RecordingStarted`：开始通知或先到的合法音频数据触发一次，不表示 SDK 可以远程开启录音。
- `RecordingFinished`：每段最多一次，`audio` 为完整或部分可用的 PCM；零帧可能为 `None`，不应生成空录音文件。原因是 `Completed / Stopped / Limit / Disconnected / Failed`。
- `Disconnected`：会话结束，原因是 `Closed / DeviceExit / Timeout / Network / System / Failed`。存在当前录音时，先交付其收尾事件。

远程确认键长按达到 1 秒阈值后，固件显示白底黑字、2px 圆角边框的顶部录音条（“正在聆听 / 松开结束”），播放设备提示音，沿用当前音量。提示音播完、喇叭关闭后才开始采集；准备期间松开会取消启动。松开结束后恢复最新远程画面，不播放结束音。提示条使用设备自带字体，不需要应用绘制；新到期的待办提醒延后到录音结束呈现。`RecordingStarted` 仍以实际音频开始到达为准，应用不需要增加接口调用。

设备提供按键交互响应时段：有效物理按键按下即提高 Wi-Fi 性能，松开后保持 3 秒，连按续期；按键事件格式和上报时机不变。到期等待当时已有图片收尾，之后的新推图、PING、查询及确认不续期。SDK 不增加热区接口，收到 `Key` 后按原图片 API 更新 UI 即可。局刷、快刷及清残影仍由设备决定。十六灰正常换图默认快刷，设备按刷新次数维护，普通待机恢复不会单独触发清残影；首次显示、切换位深或错误恢复可能耗时较长。

录音数据会重排、做丢包补偿并裁掉末帧填充。不能保证每个原始样本无损送达。先到音频、迟到开始、重复结束均在 SDK 内处理；调用者不需要再次按底层消息去重。

事件队列容量 16，未消费录音最多 2 段。持续不消费事件会结束会话并返回 `ConsumerTooSlow`；建议收到录音后交给文件工作任务，继续等待事件。

## 错误

`NoDevice`、`MultipleDevices`、`Busy`、`Disconnected`、`InvalidImage`、`InvalidAudio`、`AudioTooLong`、`InvalidOptions`、`DeviceFailure`、`ResultUnknown`、`ConsumerTooSlow`、`TransportFailure`。

`ResultUnknown` 表示可能执行过但无法确认结果，应用不得自动换请求重新播放或刷图。`Busy` 由用户再次操作或应用业务明确决定是否重试。`Disconnected` 必须创建新连接对象。

## 关闭与并发

只允许一个 `next_event` 消费任务。网络请求、保活和音频发送是独立的后台工作，等待播放完成不会阻止事件接收。命令队列有界；停止和关闭允许优先发送。

正常退出显式 `close`。`Drop` 只发出后台清理信号；运行时立即停止或进程异常终止时，以设备 15 秒保活超时作为最终释放保障。

API rustdoc 生成：`cargo doc -p zectrix-sdk --no-deps --open`。

完整画布需要配套固件和 SDK；SDK 会在连接时明确拒绝仍固定保留顶部区域的旧固件，不静默裁掉图像。此前 400×268 图片仍可输入，但会按普通图片等比例适配并居中留白，不再作为专用业务区域。

当前图片接口执行线性亮度转换、设备色板量化和误差扩散。
