# 固件升级

**简体中文** | [English](FIRMWARE.en.md) | [返回首页](../README.md)

SDK **0.2.0** 配套 **NOTE4 固件 3.7.5（简体中文 zh-CN）**，型号 `zectrix-s3-epaper-4.2`，ESP32-S3、16 MB 内置 Flash。不适用于 NOTE4C。

- [下载中文完整固件 full_3.7.5.bin](https://wiki.zectrix.com/downloads/firmware/full_3.7.5.bin)
- [在线升级页面](https://zectrix.com/firmware-updater.html) / [官方固件说明](https://wiki.zectrix.com/zh/software/firmware)
- [3.7.5 更新说明](RELEASE_NOTES_3.7.5.md)

文件大小 **5,014,576 字节**；SHA-256：

```text
212b467dbfa0f539bb041fb3d7240d7faaad6df6fc534c2e9ee8b6e99a14aa4f
```

这是从 **0x0** 写入的完整镜像，不能当作 OTA 应用镜像写入。**完整烧录会清除现有配置，启动后需要重新配网。** 升级前保存重要数据；用 Chrome 或 Edge、USB 数据线连接设备，并按官方完整镜像升级流程操作，保持供电直到校验完成。

新旧硬件使用同一中文完整镜像。带外置 Flash 的新板需要已有匹配的多语言资源；资源缺失或版本不匹配时，由设备维护方使用完整升级工具初始化。本仓库不含固件二进制、烧录工具或外置资源，不会自动刷机。

设备“设置 → 网络同步”中的“联网保持”开启时，管理端可主动连接空闲设备。关闭后设备可能断网休眠，需先在设备端选择“连接管理端”；远程会话期间仍保持联网。首次主动连接与管理端监听步骤见 [README](../README.md)。

本说明根据 2026-11-03 交付包更新；本次仓库更新未进行实机刷写或硬件验证。未来版本请核对配套清单，不仅比较版本大小。
