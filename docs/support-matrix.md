# 支持矩阵与发行版适配

本文说明 `clevo-cc-linux` 在哪些系统上可运行、依赖什么、以及不同发行版/桌面
需要额外做什么。结论先行：**用户态（clevod/CLI/UI）跨发行版；内核驱动跨内核
但只保证本机型；PolicyKit 认证代理由桌面决定，不是项目依赖。**

## 1. 组件与可移植性

| 组件 | 依赖 | 跨发行版？ |
|---|---|---|
| `clevo-proto` | 无（纯 Rust，零依赖） | ✅ 任意平台，可 `cargo test` |
| `clevo-transport` | Mock 无依赖；`acpi_call`/driver 需 Linux | ✅ 任意 Linux |
| `clevo-cc-cli` | D-Bus（zbus） | ✅ 任何有 D-Bus 的 Linux |
| `clevod` | systemd + D-Bus + polkit | ✅ 任何有 systemd/polkit 的 Linux |
| `ui/`（Tauri 2） | webkit2gtk-4.1、gtk3、libsoup3 | ✅ 主流发行版 |
| `kernel/clevo-cc` | Linux 内核 ACPI 子系统 | ✅ 跨内核可编；**协议只保证 P15 23** |

## 2. 支持等级

| 等级 | 含义 |
|---|---|
| **A · 已验证** | 真机跑通并出文档：本机 COLORFUL P15 23 + CachyOS（Arch 系） |
| **B · 应当可用** | 依赖都是通用的，理论上可用，未真机验证（其他 Arch/Debian/Fedora 等） |
| **C · 需移植** | 不同机型/模具的 `_DSM` 协议不同，需重新按 DSDT 校对 |

**机型**：目前唯一 A 级机型是 **COLORFUL P15 23**（Clevo 模具，INSYDE BIOS）。
其他同代 Clevo 机型大概率可用，但命令号/曲线偏移/能力位必须以本机 DSDT 为准
（见 `hardware-notes.md`）。

## 3. 系统依赖（按发行版）

### 3.1 用户态（clevod / CLI）

| 发行版 | 安装命令 |
|---|---|
| Arch / CachyOS | `sudo pacman -S --needed dbus polkit` |
| Debian / Ubuntu | `sudo apt install dbus polkitd` |
| Fedora / RHEL | `sudo dnf install dbus polkit` |
| openSUSE | `sudo zypper install dbus-1 polkit` |

### 3.2 内核驱动构建

需要目标内核的 headers/build 目录：

| 发行版 | 包 |
|---|---|
| Arch / CachyOS | `linux-headers`（或对应内核的 headers） |
| Debian / Ubuntu | `linux-headers-$(uname -r)` |
| Fedora | `kernel-devel` |
| openSUSE | `kernel-devel` |

本机内核用 **Clang + ThinLTO** 构建，因此编译需 `LLVM=1`：

```bash
make -C /usr/lib/modules/$(uname -r)/build M=$PWD LLVM=1
```

> 普通 GCC 内核无需 `LLVM=1`。`kernel/clevo-cc/Makefile` 会读目标内核的
> `CONFIG_CC_IS_CLANG` 自动选择，也可显式 `make LLVM=1` 覆盖（见 `install.md` §2）。

### 3.3 UI（Tauri 2）

`scripts/setup-ui-env.sh` 会检测并按发行版打印命令；手动安装：

| 发行版 | 包 |
|---|---|
| Arch / CachyOS | `webkit2gtk-4.1 gtk3 libsoup3 base-devel libayatana-appindicator` |
| Debian / Ubuntu | `libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libayatana-appindicator3-dev build-essential` |
| Fedora | `webkit2gtk4.1-devel gtk3-devel libsoup3-devel libayatana-appindicator-gtk3-devel` |

> 托盘图标在 Linux 上依赖 `libappindicator3` / `libayatana-appindicator3`；
> 缺省桌面环境（KDE/GNOME）都带 StatusNotifier 宿主，安装该库后托盘即可显示。
>
> Linux 的托盘是 AppIndicator，**只支持原生菜单、不向程序发送点击事件**（与
> Windows/macOS 不同），因此托盘菜单直接包含全部功能：
> - **性能模式** 子菜单（静音 / 节能 / 性能 / 娱乐），当前模式显示在子菜单标题上，
>   切换经 daemon 的 PolicyKit 授权，无需打开窗口；
> - **打开控制中心**、**退出**。

## 4. PolicyKit 认证代理（由桌面决定）

`clevod` **只调用标准 PolicyKit**（`org.freedesktop.PolicyKit1.Authority`），
不绑定任何具体认证代理。是否弹窗、如何弹窗由用户当前桌面的代理负责。
**没有代理时不会误放行**：交互检查会等待，`clevod` 加了 60s 超时，超时按
"授权不可用"处理并回退为仅 root（见 `hardware-notes.md` §14）。

**支持范围：KDE Plasma 与 GNOME。** 其他桌面（Hyprland/Sway/MATE/XFCE/LXQt
等）不在支持范围内，可能可以工作但未经验证。

| 桌面 | polkit 认证代理 | 状态 |
|---|---|---|
| KDE Plasma | `polkit-kde-agent` | 支持 |
| GNOME | `polkit-gnome` 或桌面内建 | 支持 |
| 无桌面 / headless | —— | 用 root 运行，或安装放行规则 |
| 其他桌面 | —— | 不在支持范围 |

> 代理属于桌面环境，不在项目打包范围内。`clevod` 只调用标准 PolicyKit，
> 弹窗由上述受支持桌面的代理负责；认证后是否跨调用缓存由代理决定。

## 5. 已知的发行版相关注意点

1. **内核编译器的 LTO**：Clang+ThinLTO 内核需 `LLVM=1`（Arch/CachyOS 常见）。
2. **udev 规则编号**：若给设备加 ACL，规则号须 < 73（`73-seat-late` 之前）。
3. **`libayatana-appindicator` 的 pkg-config 名**：Arch 上是
   `ayatana-appindicator3-0.1`，不是 `libayatana-...`。
4. **NVIDIA + WebKitGTK 的 `Gdk Error 71`**：NVIDIA 专有驱动（实测 610.57.04）
   下 WebKitGTK 2.52 分配 GBM 缓冲失败，窗口映射前即崩溃，**与合成器无关**
   （KDE/GNOME/wlroots、Wayland/X11 均重现）。UI 与 `scripts/run-ui.sh` 默认设置
   `WEBKIT_DMABUF_RENDERER_FORCE_SHM=1`：只把 DMA-BUF 传输改为共享内存，保留
   GL 合成器，因此硬件加速与毛玻璃模糊都仍然可用。**不要**用
   `WEBKIT_DISABLE_DMABUF_RENDERER=1` 作为首选——它会彻底关闭加速合成器，
   导致模糊失效、CPU 占用升高；它仅作为"软件渲染"兜底开关保留。
5. **托盘是 AppIndicator（仅菜单）**：Linux 的托盘不支持自绘弹窗，也**不向程序
   发送点击事件**（只有 Windows/macOS 会），因此托盘功能全部放在原生菜单里
   （性能模式子菜单 + 打开控制中心 + 退出）。需要 `libappindicator3` /
   `libayatana-appindicator3` 与桌面的 StatusNotifier 宿主（KDE/GNOME 自带）。
6. **systemd 是默认假设**：`clevod` 的打包文件是 systemd unit；
   非 systemd 发行版需自行写服务脚本（代码本身不依赖 systemd）。

## 6. 分发与安装（S9，已完成）

- [x] **DKMS 打包**：`kernel/clevo-cc/dkms.conf` + 自动 Clang 检测，内核升级后自动重编。
- [x] **通用安装/卸载**：`packaging/install.sh` / `uninstall.sh`（发行版无关，可回滚、
      支持 `--dry-run`/`--purge`）。
- [x] **Arch/CachyOS**：`packaging/arch/PKGBUILD` + `clevo-cc-linux.install`。
- [x] **Debian/Ubuntu**：`packaging/debian/`（debhelper + `dh-sequence-dkms`，拆三个子包）。
- [x] **udev 规则**：`packaging/udev/99-clevo-cc.rules`（`clevo-cc` 组，可选）。
- [x] **文档**：`docs/install.md`（安装/升级/卸载/排障）。

尚未做：

- [ ] rpm（Fedora/openSUSE）打包；可先用 `install.sh`。
- [ ] 其他机型（C 级）的 DSDT 校对流程文档化。
