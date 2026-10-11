# clevo-cc-linux

[English](README.en.md) | **简体中文**

在 Linux 上重写蓝天（Clevo）控制中心。基于从原厂驱动逆向出的
**DCHU / ACPI `_DSM` 协议**，提供风扇、性能模式、键盘 RGB、电池充电阈值与桌面电源集成。
该项目为 [clevo-v250rnd-linux](https://github.com/BoringNeko/clevo-v250rnd-linux) 的重新逆向实现并添加 UI 界面

> 已在 COLORFUL P15 23 / CachyOS / Fedora 44 验证通过。
> 其他机型请先按 [`docs/hardware-notes.md`](docs/hardware-notes.md) 核对 DSDT。
> 并且不保证其余蓝天公模可用。

## UI 预览

![概览](https://r2.boring520.top/file/clevo/1791721295919_overview.png)

![风扇](https://r2.boring520.top/file/clevo/1791721296723_fan.png)

![RGB](https://r2.boring520.top/file/clevo/1791721191886_RGB.png)

![电源](https://r2.boring520.top/file/clevo/1791721217689_power.png)

![设置](https://r2.boring520.top/file/clevo/1791721188766_settings.png)

## 特性

- **只读监控**：风扇转速（CPU / GPU1）、CPU/GPU 温度（摄氏度）、风扇曲线。
- **写入控制**：风扇模式（`auto` / `quiet` / `max` / `maxq` / `customize`，
  UI 中显示为 `customize`，即固件/CLI 的 `custom`）、
  **自定义四段风扇曲线**（命令 14，写入后自动切到该模式；曲线卡片只在
  `customize` 模式下显示，两条折线都可拖，带「保存 / 还原配置 / 还原默认」
  三个按钮）与性能模式
  （`quiet` / `pwrsaving` / `performance` / `entertainment`），
  经内核驱动 + PolicyKit 授权，可逆。
- **能力发现**：绕过不可用的 AppSettings EC page 0..7，通过 GCMD
  `122/96/16/60/82` 提供 `capabilities` 和 `profile list` 数据；性能模式写入前
  校验支持位，无法恢复的字段保持未知。
- **跨重启保留**：`clevod` 在每次成功写入后把风扇模式、性能模式、自定义曲线、
  键盘配置和充电阈值
  存进 `/etc/clevo-cc/clevod.toml`，启动时重放（EC 断电即忘，重放先下发曲线再
  切 `custom`）。「还原默认」用的是守护进程在**冷启动、写入之前**从 EC 快照到
  的**本机真实出厂曲线**，不是硬编码表；热重启时不会把用户曲线误当成出厂值。
- **键盘 RGB**：根据硬件启用 ITE `048d:8910` USB HID 逐键控制，或本机
  RGB15 ACPI-DCHU 单区控制；本机同时提供 KDE/PowerDevil 可识别的
  `clevo::kbd_backlight` 标准亮度接口（原始范围 `0..191`），颜色切换带约
  0.3 秒的软件渐变，配置由 `clevod` 持久化。
- **守护进程**：`clevod` 是唯一接触硬件的长期进程，在系统 D-Bus 上提供
  `org.clevo.CC`，缓存读数并持久化用户选择。
- **电池与桌面电源**：电源页面显示真实电量、充电状态、健康度和循环次数；
  标准充电阈值和原生 FlexiCharger 均可经 PolicyKit 保存并在启动时恢复；原生通道
  支持启用开关和固件档位选择，缺少可用接口时明确显示不可用。
  本机已通过 D-Bus 设置并回读启用的 40–80% 窗口；完整充电周期和重启恢复待真机验收。
  内核 `platform_profile` 与四种性能模式共用写入路径，桌面切换会同步到 daemon。
  使用和真机验收见 [`docs/power.md`](docs/power.md)。
- **桌面 UI**：React 前端，Tauri 2（默认）或 Electron 两个壳均可运行，只经 D-Bus
  通信；玻璃拟态仪表盘、自绘标题栏、概览 / 风扇 / RGB / 电源与电池页面、CPU/GPU 硬件占用、内存与
  zram/swap、硬盘挂载占用、可拖拽编辑的风扇曲线、深浅色、自定义强调色/logo/壁纸、
  自绘取色器、显示设置（比例/分辨率/缩放，重启记忆）、动画速度与开关、兼容性选项。
  Electron 用于规避 WebKitGTK 在 NVIDIA 驱动上的崩溃，见
  [`docs/electron.md`](docs/electron.md)。
- **系统托盘**：原生菜单直接切换性能模式（`▶` 标记当前模式）、打开控制中心、退出；
  关闭窗口即回到托盘，进程继续常驻。
- **UI 登录自启动**：设置 / 应用程序中可开关，登录时仅驻留托盘；支持 Tauri 与
  Electron，配置属于当前用户，不需要 root。见 [`docs/autostart.md`](docs/autostart.md)。
- **只读诊断**：`clevo-cc --transport dbus doctor` 检查硬件、服务、权限和配置，
  连接失败也输出完整报告与修复建议；支持 `--json`。见 [`docs/doctor.md`](docs/doctor.md)。
- **安全默认**：写入默认关闭；`acpi_call` 传输只读；未验证的固件常量明确标注。
- **可离线测试**：无需硬件，协议 fixture、隔离 sysfs 和私有 D-Bus 覆盖读写与拒绝路径。

> 风扇控制（转速、温度、曲线读写、风扇/性能模式）已在真机上逐项验证，
> 记录见 [`docs/hardware-notes.md`](docs/hardware-notes.md)；
> 写入相关的实测细节见 §7.2 与 §10.4。

## 架构

```
UI (Tauri/Electron + React) ─┐
CLI (--transport dbus)      ─┴─▶ org.clevo.CC (D-Bus, PolicyKit) ─▶ clevod
                                                               ├─▶ 内核驱动 / acpi_call ─▶ 固件
                                                               └─▶ power_supply / platform_profile
```

- UI 和 `--transport dbus` 的 CLI 经 daemon 访问硬件；`driver` / `acpi-call`
  可用于直接访问与诊断。充电阈值的 CLI 写入只支持 D-Bus。
- D-Bus 写入经 PolicyKit；阈值写入核对回读，失败时尝试恢复原配置。

## 技术栈

| 部分 | 技术 |
|---|---|
| 协议层 / 传输 / CLI / 守护进程 | Rust（`zbus`、`tokio`、`serde`） |
| 内核驱动 | C（ACPI platform driver，GPL-2.0-only，DKMS） |
| 桌面 UI | React + TypeScript + Vite + MUI；壳可选 Tauri 2 或 Electron |
| 集成 | D-Bus、PolicyKit、systemd、udev、DKMS |

## 安装

```bash
sudo packaging/install.sh                 # 装驱动(DKMS)+守护进程+D-Bus/polkit/systemd/udev/man
sudo packaging/install.sh --enable        # 并立即启动 clevod
sudo packaging/install.sh --enable --ui   # 立即启动 clevod 并安装 Tauri 2 UI
sudo packaging/install.sh --enable --electron  # 改用 Electron UI（NVIDIA 驱动推荐）
sudo packaging/uninstall.sh               # 卸载应用与系统集成；不恢复之前的充电策略
```

先用 `packaging/install.sh --dry-run` 预览。发行版包（deb / rpm / AppImage）
用 `packaging/build-packages.sh` 构建（产物在 `dist/`）；另有
`packaging/arch/PKGBUILD`（Arch/CachyOS）。
完整说明（选项、升级、免 root 访问、排障）见
[`docs/install.md`](docs/install.md)。

## 电源快速检查

```bash
clevo-cc --transport dbus power status --json
# 仅在 status 确认 BAT0 原生档位支持 40/80 时使用；默认仅预览。
clevo-cc --transport dbus power set-thresholds BAT0 --start 40 --end 80 --enabled true
# 确认后追加 --apply，经 PolicyKit 设置。
```

UI 的「电源与电池」页左侧显示外接电源与桌面模式，右侧显示电池与阈值控件。
阈值不会把已有的高电量主动降到停止百分比。KDE 的外接电源图标与实际充电状态
可能不同，排查方法见 [`docs/power.md`](docs/power.md)。

## 文档

| 文档 | 内容 |
|---|---|
| [`docs/hardware-notes.md`](docs/hardware-notes.md) | 逆向出的协议事实与真机验证 |
| [`docs/install.md`](docs/install.md) | 安装 / 升级 / 卸载 |
| [`docs/doctor.md`](docs/doctor.md) | 只读诊断、JSON 报告与退出码 |
| [`docs/power.md`](docs/power.md) | 电池状态、充电阈值和桌面电源模式 |
| [`docs/support-matrix.md`](docs/support-matrix.md) | 支持矩阵与发行版适配 |
| [`docs/desktop-ui.md`](docs/desktop-ui.md) | 桌面 UI 页面、设置与托盘使用说明 |
| [`docs/autostart.md`](docs/autostart.md) | UI 登录自启动与仅驻留托盘 |
| [`docs/electron.md`](docs/electron.md) | Electron 壳：架构、构建与安装 |

## 补充

项目使用 Opencode + Deepseek-V4.1-Flash 完成

## 许可证

分治许可：`kernel/` 为 **GPL-2.0-only**；其余为 **MIT OR Apache-2.0**。
见 [`LICENSES/`](LICENSES/)。

本项目未复制任何原厂代码；`ControlCenter-RE/` 下的逆向参考仅供参考。
