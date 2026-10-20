# Firmware update

[简体中文](FIRMWARE.md) | **English** | [Home](../README.en.md)

Official update route: [ZECTRIX Wiki](https://wiki.zectrix.com/zh/software/firmware) → Note 4 → [ZECTRIX Cloud Local Upgrade](https://cloud.zectrix.com/). Firmware and flashing tools are not distributed here.

This SDK snapshot matches **Note 4 firmware 3.7.2**. This version supports the palette and grayscale protocol. Check explicit compatibility guidance for future versions.

1. Sign in to ZECTRIX Cloud in Chrome or Edge and open **Local Upgrade** (本地升级).
2. Connect Note 4 using a USB data cable. Choose **Connect device** (连接设备) and select its serial port.
3. Select the matching Note 4 firmware and click **Load** (加载). Check the model, version and any language options required by the page.
4. To retain Wi-Fi and other configuration, enable **Keep device data** (保留设备数据) where available.
5. Choose **Start flashing** (开始刷写), wait for explicit completion in the terminal, then restart. Keep USB connected, the page open and the computer awake throughout.
6. After updating, enable **Keep Wi-Fi on during standby** (待机保持 Wi-Fi) under **Settings → Network Sync** (设置 → 网络同步) on the device.

If the required version is unavailable, confirm a compatible version with official support. Never substitute Note 4C firmware. This SDK repository does not perform firmware updates.

Checked on 2026-10-19. The current official Wiki and Cloud UI remain authoritative for labels and available versions.
