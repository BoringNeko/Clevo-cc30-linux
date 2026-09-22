# clevo-cc-linux

[English](README.en.md) | **简体中文**

在 Linux 上重写蓝天（Clevo）控制中心。基于从原厂驱动逆向出的
**DCHU / ACPI `_DSM` 协议**，实现风扇转速监控与风扇/性能模式控制。
该项目为 [clevo-v250rnd-linux](https://github.com/BoringNeko/clevo-v250rnd-linux) 的重新逆向实现并添加 UI 界面

> 已在 COLORFUL P15 23 / CachyOS / Fedora 44 验证通过。
> 其他机型请先按 [`docs/hardware-notes.md`](docs/hardware-notes.md) 核对 DSDT。
> 并且不保证其余蓝天公模可用。

## 特性

- **只读监控**：风扇转速（CPU / GPU1）、CPU/GPU 温度（摄氏度）、风扇曲线。
- **写入控制**：风扇模式（`auto` / `quiet` / `max` / `maxq` / `customize`，
  UI 中显示为 `customize`，即固件/CLI 的 `custom`）、
  **自定义四段风扇曲线**（命令 14，写入后自动切到该模式；曲线卡片只在
  `customize` 模式下显示）与性能模式
  （`quiet` / `pwrsaving` / `performance` / `entertainment`），
  经内核驱动 + PolicyKit 授权，可逆。
- **守护进程**：`clevod` 是唯一接触硬件的长期进程，在系统 D-Bus 上提供
  `org.clevo.CC`，缓存读数并持久化用户选择。
- **桌面 UI**：Tauri 2 应用，只经 D-Bus 通信；玻璃拟态仪表盘、自绘标题栏、
  可拖拽编辑的风扇曲线、深浅色、自定义强调色/logo/壁纸、自绘取色器、
  显示设置（比例/分辨率/缩放，重启记忆）、兼容性选项。
- **系统托盘**：原生菜单直接切换性能模式（`▶` 标记当前模式）、打开控制中心、退出；
  关闭窗口为隐藏到托盘，进程继续常驻。
- **安全默认**：写入默认关闭；`acpi_call` 传输只读；未验证的固件常量明确标注。
- **可离线测试**：无需硬件，全部用手写 fixture 测试（163 Rust + 25 UI 后端 + 127 前端）。

> 风扇控制（转速、温度、曲线读写、风扇/性能模式）已在真机上逐项验证，
> 记录见 [`docs/hardware-notes.md`](docs/hardware-notes.md)；
> 写入相关的实测细节见 §7.2 与 §10.4。

## 架构

```
UI (Tauri/React)  ─┐
CLI               ─┼─▶ org.clevo.CC (D-Bus, PolicyKit) ─▶ clevod ─▶ 内核驱动 / acpi_call ─▶ EC
```

- 只有 `clevod` 访问硬件；CLI 与 UI 都是 D-Bus 客户端。
- 写操作单点、经 PolicyKit、可回滚。

## 技术栈

| 部分 | 技术 |
|---|---|
| 协议层 / 传输 / CLI / 守护进程 | Rust（`zbus`、`tokio`、`serde`） |
| 内核驱动 | C（ACPI platform driver，GPL-2.0-only，DKMS） |
| 桌面 UI | Tauri 2 + React + TypeScript + Vite + MUI |
| 集成 | D-Bus、PolicyKit、systemd、udev、DKMS |

## 安装

```bash
sudo packaging/install.sh                 # 装驱动(DKMS)+守护进程+D-Bus/polkit/systemd/udev/man
sudo packaging/install.sh --enable        # 并立即启动 clevod
sudo packaging/install.sh --enable --ui   # 立即启动 clevod 并安装 UI 界面
sudo packaging/uninstall.sh               # 完整回滚
```

先用 `packaging/install.sh --dry-run` 预览。发行版包（deb / rpm / AppImage）
用 `packaging/build-packages.sh` 构建（产物在 `dist/`）；另有
`packaging/arch/PKGBUILD`（Arch/CachyOS）。
完整说明（选项、升级、免 root 访问、排障）见
[`docs/install.md`](docs/install.md)。

## 文档

| 文档 | 内容 |
|---|---|
| [`docs/hardware-notes.md`](docs/hardware-notes.md) | 逆向出的协议事实与真机验证 |
| [`docs/install.md`](docs/install.md) | 安装 / 升级 / 卸载 |
| [`docs/support-matrix.md`](docs/support-matrix.md) | 支持矩阵与发行版适配 |

## 补充

项目使用 Opencode + Deepseek-V4.1-Flash 完成

## 许可证

分治许可：`kernel/` 为 **GPL-2.0-only**；其余为 **MIT OR Apache-2.0**。
见 [`LICENSES/`](LICENSES/)。

本项目未复制任何原厂代码；`ControlCenter-RE/` 下的逆向参考仅供参考。
