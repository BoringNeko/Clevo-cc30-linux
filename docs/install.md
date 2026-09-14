# 安装、升级与卸载

本文档说明如何在真机上安装 `clevo-cc-linux`（守护进程 + CLI + 内核驱动 +
可选的桌面 UI），以及如何干净地回到安装前的状态。

> **适用范围**：唯一 A 级机型是 **COLORFUL P15 23**（Clevo 模具，INSYDE BIOS）。
> 其他机型请先按 `hardware-notes.md` 校对 DSDT。支持等级见 `support-matrix.md`。
>
> **已验证**：本安装流程已在 COLORFUL P15 23 + CachyOS 上跑通（DKMS / systemd /
> D-Bus / PolicyKit / udev / CLI / 写入）。

安装会改动系统（装入内核模块、systemd 单元、D-Bus/PolicyKit/udev 配置）。
**所有步骤都可以用 `packaging/uninstall.sh` 完整回滚**，且安装时默认不会
写入硬件、不会自动启用服务（除非显式 `--enable`）。

---

## 1. 快速安装（任意发行版）

前置依赖（Arch/CachyOS 示例）：

```bash
sudo pacman -S --needed base-devel dkms linux-headers dbus polkit
```

从源码目录安装：

```bash
cd clevo-cc-linux
sudo packaging/install.sh            # 装驱动 + 守护进程 + D-Bus/polkit/udev/手册
sudo packaging/install.sh --enable   # 顺带启用并启动 clevod
```

安装内容：

| 路径 | 内容 |
|---|---|
| `/usr/bin/clevod` | 守护进程（唯一的硬件访问者） |
| `/usr/share/dbus-1/system.d/org.clevo.CC.conf` | D-Bus 策略 |
| `/usr/share/polkit-1/actions/org.clevo.CC.policy` | 写操作的 PolicyKit 授权 |
| `/usr/lib/systemd/system/clevod.service` | systemd 单元（使用 `--driver`） |
| `/usr/lib/udev/rules.d/99-clevo-cc.rules` | 免 root 访问 sysfs（可选） |
| `/usr/share/man/man8/clevod.8`、`man1/clevo-cc.1` | 手册页 |
| `/usr/src/clevo-cc-<ver>/` + DKMS | 内核驱动源码 |

### 常用选项

```
--prefix DIR     安装前缀（默认 /usr）
--version VER    DKMS 版本号（默认取 Cargo.toml）
--bin-dir DIR    使用 DIR 里预编译好的 clevod/clevo-cc，跳过 cargo 构建
--no-driver      不装内核驱动（只用只读 acpi_call 回退）
--no-udev        不装 udev 规则、不建 clevo-cc 组
--ui             额外安装已构建的桌面 UI 二进制
--enable         安装后 systemctl enable --now clevod
--dry-run        只打印将要执行的操作
```

> 构建使用**当前用户**的 Rust 工具链（遵循标准的 `CARGO_HOME` / `RUSTUP_HOME`），
> 不假设任何特定账号。若当前用户没有 Rust，请先自行
> `cargo build --release -p clevod -p clevo-cc-cli`，再用 `--bin-dir` 指向产物。
> 内核驱动不受影响：它始终由 DKMS 针对本机内核现场编译。

先看一遍会做什么：

```bash
packaging/install.sh --dry-run
```

安装后启动并检查：

```bash
sudo systemctl enable --now clevod
systemctl status clevod
# 通过系统 D-Bus 读取（无需 root）：
clevo-cc --transport dbus fan status
```

### 离线自测（不碰硬件、无需安装）

CLI 默认连**系统总线**；要连在私有 session bus 上跑的测试 daemon，加
`--dbus-session`（daemon 用 `--session-bus --mock`）：

```bash
dbus-run-session -- sh -c '\
  clevod --session-bus --mock crates/clevod/tests/fixtures/test.fixture & \
  sleep 1; clevo-cc --transport dbus --dbus-session fan status'
```

---

## 2. 内核驱动（DKMS）

驱动通过 DKMS 安装到 `/usr/src/clevo-cc-<ver>`，每次内核升级后自动重编。
驱动不进入 initramfs（`AUTOINSTALL="no"`），避免坏构建影响启动。

构建时 `kernel/clevo-cc/Makefile` 会**自动检测**目标内核是否由 Clang 构建
（读 `CONFIG_CC_IS_CLANG`），因此 CachyOS 这类 `Clang + ThinLTO` 内核无需手工
传 `LLVM=1`。若检测不准，可显式覆盖：

```bash
# 手动构建（不经过 DKMS）
cd kernel/clevo-cc
make LLVM=1     # 或 make 强制 GCC
```

验证驱动：

```bash
sudo modprobe clevo-cc
dmesg | tail
cat /sys/devices/platform/CLV0001:00/fan_mode    # auto/quiet/max/maxq
cat /sys/class/hwmon/hwmon*/fan1_input           # CPU rpm
echo max | sudo tee /sys/devices/platform/CLV0001:00/fan_mode
```

若 `_DSM` 返回 `0x80000002`，驱动仍会加载但读取返回 `-EOPNOTSUPP`，查 `dmesg`。

---

## 3. 免 root 访问（可选）

默认 `clevod` 以 root 运行，普通用户经 D-Bus + PolicyKit 操作，**不需要** udev
规则。仅当你希望直接读写 sysfs（脚本、hwmon 工具）时才启用：

```bash
# install.sh 已创建 clevo-cc 组，把用户加进去：
sudo usermod -aG clevo-cc "$USER"
# 重新登录后生效
```

udev 规则会把 `fan_mode`/`perf_mode` 设为 `clevo-cc` 组可读写（`0660`），
hwmon 节点设为组可读。**加入该组等同授予写入 EC 的能力，请视为特权。**

---

## 4. 桌面 UI

UI 是 Tauri 2 应用，需要 webkit 运行库：

```bash
# Arch
sudo pacman -S --needed webkit2gtk-4.1 gtk3

# 构建（生成 ui/src-tauri/target/release/clevo-cc-ui）
cd ui && pnpm install && pnpm tauri build

# 安装二进制 + 桌面项 + 图标
cd .. && sudo packaging/install.sh --ui
```

UI 只通过 D-Bus 读取/控制，不直接接触硬件。

---

## 5. 发行版包

三种格式（deb / rpm / AppImage）可用一个脚本构建，产物在 `dist/`：

```bash
packaging/build-packages.sh            # 全部
packaging/build-packages.sh deb rpm    # 只构建指定格式
```

依赖：deb 需 `dpkg`（`dpkg-deb`），rpm 需 `rpm-tools`（`rpmbuild`），
AppImage 需 `appimagetool`（放到 `packaging/appimage/`，或已在 PATH 中）；
构建用户态二进制需要 `cargo`，构建 UI 需要 `pnpm` + webkit 栈。

推送 `v*` 形式的 tag 时，`.github/workflows/release.yml` 会在 CI 中构建三种包
并自动发布到 GitHub Release；也可在 Actions 里手动触发。

### 5.1 deb（Debian / Ubuntu）

直接组装，不依赖 `debhelper`：

```
clevo-cc-linux_<ver>_amd64.deb        守护进程 + CLI + D-Bus/polkit/systemd/udev/man
clevo-cc-linux-dkms_<ver>_amd64.deb   内核驱动源码，装时由 DKMS 编译
clevo-cc-linux-ui_<ver>_amd64.deb     UI（仅在已构建时产出）
```

```bash
sudo apt install ./clevo-cc-linux_*.deb ./clevo-cc-linux-dkms_*.deb
# 或一次装齐（ui 可选，主包依赖 dkms，会自动带上）：
sudo apt install ./clevo-cc-linux_*.deb ./clevo-cc-linux-ui_*.deb
```

`packaging/debian/` 另有一份标准的 `debhelper` 打包源，供在真正的 Debian
环境里用 `dpkg-buildpackage` 构建。

### 5.2 rpm（Fedora / openSUSE）

单个 RPM 含驱动源码、`clevod`、CLI 与 UI；`%post` 调 DKMS 为本机内核编译驱动：

```bash
sudo dnf install ./clevo-cc-linux-*.rpm
```

### 5.3 AppImage（仅 UI）

AppImage 是自包含、免安装格式，**无法**装内核模块、systemd 服务或 polkit
策略，因此**只打包 UI**。运行前系统里需已有 `clevod` 后端：

```bash
./clevo-cc-ui-<ver>-x86_64.AppImage
```

### 5.4 Arch / CachyOS（PKGBUILD）

```bash
cd packaging/arch && makepkg -si
```

### 5.5 其他发行版

直接用 `packaging/install.sh`（发行版无关）；只需保证 `dkms`、目标内核的
headers、`dbus`、`polkit` 存在。

---

## 6. 升级

- **源码安装**：`git pull` 后重跑 `sudo packaging/install.sh`。脚本是幂等的；
  DKMS 会先 `remove` 旧版本再安装新版本。
- **Arch**：更新 `pkgver`/`sha256sums` 后 `makepkg -si`；`post_upgrade` 会
  重载 udev 与 systemd。
- **版本号一致性**：DKMS 版本取自工作区 `Cargo.toml` 的 `version`，请统一
  维护。

升级后：

```bash
sudo systemctl restart clevod
```

---

## 7. 卸载

完整回滚（保留 `/etc/clevo-cc` 与 `clevo-cc` 组）：

```bash
sudo packaging/uninstall.sh
```

连同配置与组一起清除：

```bash
sudo packaging/uninstall.sh --purge
```

卸载脚本会停用并删除服务、二进制、D-Bus/polkit/udev 配置、手册页、桌面项，
并 `dkms remove` 内核模块。常用选项：

```
--prefix DIR   安装时使用的前缀
--version VER  DKMS 版本号
--purge        同时删除 /etc/clevo-cc 与 clevo-cc 组
--dry-run      只打印
```

包管理器安装的用其自身方式卸载（`pacman -Rns clevo-cc-linux`、
`apt remove clevo-cc-linux`）；`PKGBUILD`/debian 的 `post_remove`/`prerm`
会处理 DKMS 与服务。

---

## 8. 故障排查

| 现象 | 排查 |
|---|---|
| `clevod` 启动失败 | `journalctl -u clevod -b`；确认 `--driver` 时模块已加载，否则回退 acpi_call |
| UI 无数据 | `busctl --system status org.clevo.CC`；确认服务在系统总线 |
| 写入无反应/报错 | 无桌面认证代理时写入仅限 root；KDE/GNOME 才有弹窗（见 `support-matrix.md`） |
| 风扇模式不生效 | 别是 `silent(3)`（空实现）或 `custom(6)`（需先写曲线） |
| DKMS 未随内核重编 | `dkms status`；确认 `linux-headers` 与 DKMS 服务已启用 |

---

## 9. 安全说明

- 写入默认关闭：`acpi_call` 传输只读，Mock 传输永不写。
- 只有 `clevod` 访问硬件；CLI/UI 只是 D-Bus 客户端。
- `clevo-cc` 组成员可直接写 sysfs，属特权操作。
- 驱动为 GPL-2.0-only，其余为 MIT OR Apache-2.0。
