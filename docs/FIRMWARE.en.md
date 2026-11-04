# Firmware update

[简体中文](FIRMWARE.md) | **English** | [Home](../README.en.md)

SDK **0.2.0** is paired with **NOTE4 firmware 3.7.5 (Simplified Chinese, zh-CN)** for `zectrix-s3-epaper-4.2`, ESP32-S3 with 16 MB internal flash. This package does not support NOTE4C.

- [Download full_3.7.5.bin](https://wiki.zectrix.com/downloads/firmware/full_3.7.5.bin)
- [Web updater](https://zectrix.com/firmware-updater.html) / [Official firmware guide](https://wiki.zectrix.com/zh/software/firmware)
- [3.7.5 release notes (Chinese)](RELEASE_NOTES_3.7.5.md)

Size: **5,014,576 bytes**. SHA-256:

```text
212b467dbfa0f539bb041fb3d7240d7faaad6df6fc534c2e9ee8b6e99a14aa4f
```

This is a full image written at **0x0**, not an OTA application image. **Full flashing clears existing configuration; configure Wi-Fi again after boot.** Save important data first. Use Chrome or Edge and a USB data cable, follow the official full-image flashing procedure, and keep power connected until verification completes.

Old and new hardware use the same Chinese full image. New boards with external flash require matching multilingual resources to be present. If resources are missing or incompatible, the device maintainer must initialize them using the complete upgrade tools. This source repository includes no firmware binary, flashing tool or external resource, and never flashes automatically.

With “Keep connected” (联网保持) enabled under Settings → Network Sync, the manager can connect to an idle device. With it disabled, the device may sleep offline; choose “Connect to manager” on the device. Remote sessions stay connected. See [README](../README.en.md) for first-time connection and listener setup.

Updated from the 2026-11-03 delivery. No hardware flashing or device validation was performed for this repository update. Check matching firmware metadata rather than assuming compatibility from version ordering.
