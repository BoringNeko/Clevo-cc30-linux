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

### CPU 温度（默认不需要换算）

命令 12 的 CPU 温度在偏移 `[18]`。原厂会用 CPU 型号去 `cpu.ini` 查 TDP 档位
再做分段换算，**查不到就不换算**。

实测 COLORFUL P15 23 属"查不到"那类：原始字节本身就是摄氏度（`raw 52` vs
`sensors 54°C`，`raw 88` vs `87°C`）。**所以默认不做任何换算。**

只有确认你的 CPU 确实属于原厂 `cpu.ini` 的某一档时，才在
`/etc/clevo-cc/clevod.toml` 里设置：

```toml
cpu_tdp_class = "35W"   # 或 47W / 65W / 84W / 91W
```

**设错档位会让读数变差**（本机设成 47W 会把 54°C 变成 39°C、87°C 变成
57°C）。不填即不换算，是安全默认。GPU 温度始终是直接摄氏度。

CLI 可临时覆盖：`CLEVO_TDP_CLASS=65W clevo-cc --transport driver fan status`。

验证方式：与 `sensors` 的 `Package id 0` 对照，差距应在几度以内。

### 自定义风扇曲线
```bash
# 先看当前曲线（只读）
clevo-cc --transport dbus fan curve

# 预演要写入的内容（不碰硬件）
clevo-cc --transport dbus fan set-curve --cpu "40,20 60,40 80,70 100,100"

# 真正写入并切换到 custom 模式（触发 PolicyKit 授权）
clevo-cc --transport dbus fan set-curve --cpu "40,20 60,40 80,70 100,100" --apply
```

`--gpu1` 省略时沿用 CPU 曲线；`--gpu2` 省略时为全零（表示"不动这个通道"）。
温度必须严格递增，占空比 `0..100`。写入后会自动切到 `custom` 模式，
否则固件不会使用新曲线。想恢复自动控制：`clevo-cc --transport dbus fan set-mode auto --apply`。

> **底层语义**（排查时有用）：命令 14 是**整表替换**。内核驱动会先读当前曲线、
> 只合并你点名的通道，再整份下发，所以只改 CPU 不会碰 GPU。详见
> [`hardware-notes.md` §7.2](hardware-notes.md)。
>
> 直接写 sysfs 时请**每次只写一个通道**并读回校验：多行 `printf > fan_curve`
> 会被 shell 拆成多次 `write()`，而退出码只反映最后一次。

### 更新已装的内核驱动与守护进程

改过 `kernel/` 或 Rust 代码之后，**必须重装**才会生效：

```bash
sudo packaging/install.sh            # DKMS 模块 + clevod/clevo-cc + UI
sudo rmmod clevo_cc && sudo modprobe clevo_cc   # 立即换成新模块
cat /sys/module/clevo_cc/srcversion  # 与内核目录下的 .ko 比对
modinfo -F srcversion kernel/clevo-cc/clevo-cc.ko
```

两个 `srcversion` 一致即表示加载的是最新构建。

**`install.sh` 会自动重启正在运行的 `clevod`**（打印
`restarting clevod to pick up the new binary`）。这一点很关键：`systemctl
enable --now` 对已在运行的服务是空操作，换了二进制却不重启，内存里跑的还是
旧代码，而症状是**报错指向源码而非旧进程** —— 例如新属性读成
`Unknown property`、新支持的模式被拒。

> 排查时先看进程启动时间，它比二进制旧就说明没重启：
>
> ```bash
> systemctl show clevod -p ActiveEnterTimestamp --value
> stat -c %y /usr/bin/clevod
> ```
>
> 手动重启：`sudo systemctl restart clevod`。

**内核模块与用户态是分开的**：只改 Rust（`clevod` / CLI / UI）时，重装即可，
`.ko` 未变、`srcversion` 也不会变。

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

### 分辨率的处理方式

界面按 **1600×900** 设计：所有尺寸（卡片、内边距、字体）都以这个逻辑视口为
基准，再整体等比缩放填满实际窗口，因此**任何窗口尺寸都不会出现滚动条**。

设置 → 显示 里的“分辨率”选择的是**窗口大小**，用于控制界面缩放的基准；
缩放倍率取 `min(实际宽/1600, 实际高/900)`，所以非 16:9 的窗口会留黑边而不是
裁切。默认窗口即 1600×900。

> 注意：系统缩放（如 `Xft.dpi=129`）会让 1600×900 的窗口只有约 1185×666 的
> 逻辑像素。这是预期行为——界面会按比例缩小以完整显示，不需要额外设置。

> 托盘图标需要 `libappindicator3` / `libayatana-appindicator3`；各发行版包名见
> [`support-matrix.md`](support-matrix.md) §3.3。

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
| 风扇模式不生效 | 别是 `silent(3)`（空实现）；`custom(6)` 需要先写入曲线 |
| 自定义曲线不生效 | 写曲线后**必须**再切到 `custom` 模式（CLI/UI 会自动切换；直接写 sysfs 时需 `echo custom > fan_mode`） |
| 曲线被 EC 拒绝 | 温度必须严格递增、占空比 0–100；`dmesg` 会记录 `_DSM` 的失败原因 |
| 温度显示 `n/a` | 该通道 EC 未上报（原始值为 0）；不代表 0°C，属正常 |
| CPU 温度不对/偏高 | 配置里 `cpu_tdp_class` 必须是本机 CPU 的 TDP 档（`35W`/`47W`/`65W`/`84W`/`91W`）。默认 `47W`（P15 23）；写错档位会得到错误的 CPU 温度 |
| 想核对原始字节 | `cat /sys/devices/platform/CLV0001:00/raw_status`（命令 12 的原始 hex） |
| DKMS 未随内核重编 | `dkms status`；确认 `linux-headers` 与 DKMS 服务已启用 |

---

## 9. 安全说明

- 写入默认关闭：`acpi_call` 传输只读，Mock 传输永不写。
- 只有 `clevod` 访问硬件；CLI/UI 只是 D-Bus 客户端。
- `clevo-cc` 组成员可直接写 sysfs，属特权操作。
- 驱动为 GPL-2.0-only，其余为 MIT OR Apache-2.0。

---

## 10. 测试

测试分三层：**完全离线** → **mock daemon 端到端** → **真机**。前两层
不需要任何硬件，第三层按风险从只读排到写入。

### 10.1 离线（CI 等价）

```bash
scripts/run-tests.sh              # 全部
scripts/run-tests.sh --rust       # 只跑 Rust
scripts/run-tests.sh --ui         # 只跑 UI（类型检查 + 前端 + 后端）
scripts/run-tests.sh --kernel     # 只编译内核模块
```

它会依次跑 `cargo fmt --check`、`clippy -D warnings`、workspace 测试
（含 `dbus-run-session` 的 D-Bus 集成测试）、前端类型检查与 vitest、
UI 后端测试、内核模块编译。与 `.github/workflows/ci.yml` 一致，所以
本地通过即 CI 通过。

单跑某一层：

```bash
dbus-run-session -- cargo test --workspace        # Rust，含 D-Bus（推荐）
cargo test --workspace                            # 无 session bus 时 D-Bus 测试自动跳过
cd ui && pnpm test && pnpm typecheck
cd ui/src-tauri && cargo test
make -C kernel/clevo-cc                            # 需内核 headers
```

> Rust 与 UI 测试全部基于手写 fixture，**不会碰硬件**。`dbus-run-session`
> 提供私有 session bus，让 D-Bus 集成测试真的走一遍 `org.clevo.CC` 的线协议
> （属性读取、方法调用、PolicyKit 拒绝路径、信号）。

### 10.2 mock daemon 端到端（不碰硬件，但走真实 D-Bus）

```bash
dbus-run-session -- sh -c '
  ./target/release/clevod --session-bus --mock crates/clevod/tests/fixtures/test.fixture &
  sleep 1
  ./target/release/clevo-cc --transport dbus --dbus-session fan status
  ./target/release/clevo-cc --transport dbus --dbus-session fan curve
  ./target/release/clevo-cc --transport dbus --dbus-session fan set-curve --cpu "40,20 60,40 80,70 100,100"
  ./target/release/clevo-cc --transport dbus --dbus-session fan set-curve --cpu "40,20 60,40 80,70 100,100" --apply
'
```

最后一条会返回 `AccessDenied`（非 root、无 polkit 代理），这是**正确行为** ——
它证明 PolicyKit 门确实生效了。以 root 或装了放行规则的桌面跑则是成功写入。

### 10.3 真机（需要硬件，按风险递增）

```bash
sudo scripts/verify-hardware.sh --step 1   # 只读：acpi_call 读状态/曲线（最安全）
sudo scripts/verify-hardware.sh --step 2   # 只读：驱动 hwmon 转速/温度 + sysfs 曲线
sudo scripts/verify-hardware.sh --step 3   # 可逆：风扇模式 auto→max→auto
sudo scripts/verify-hardware.sh             # 全部（含曲线写入往返，会先备份再还原）
```

曲线写入的压力测试（多轮写→读回→校验，结尾做内核健康检查）：

```bash
sudo scripts/curve-test.sh 10    # 10 轮；每轮写 cpu+gpu1 各一次并校验
```

它每行单独写、单独校验，所以失败能指名通道；结尾会检查
`usercopy_abort` / `kernel BUG` / `Oops` / `ACPI Error`，**四项都应为 0**。

脚本每步都会先打印将要做什么并征求确认；第 4 步会**先保存当前曲线**，写入
测试曲线后读回比对，最后还原，并把风扇模式留在 `auto`（即使还原被跳过，固件
也始终保有控制权）。

真机上需要重点确认的三件事（**均已在 COLORFUL P15 23 上验证通过**）：

| 检查点 | 位置 | 期望 | 实测结果 |
|---|---|---|---|
| 温度是否可信 | step 1/2 的温度 | 与 `sensors` 一致 | ✅ 负载时 87 = 87 °C |
| 温度通道 | `temp1_input` | GPU 温度（m°C） | ✅ `n/a` 表示 EC 未上报 |
| 曲线写入是否被接受 | 读回比对 | 读回的点与写入一致 | ✅ 10 轮压测全过 |

> CPU 温度由 `clevod` 换算（见上文），驱动的 `temp*_input` 只暴露 GPU；
> 若读数明显偏离 `sensors`，检查 `cpu_tdp_class` 配置。

### 10.4 排障

| 现象 | 排查 |
|---|---|
| D-Bus 测试被跳过 | 用 `dbus-run-session -- cargo test`；直接 `cargo test` 时会自动跳过 |
| 内核模块编译失败 | 装内核 headers（`linux-headers` / `kernel-devel`）；CachyOS 这类 Clang+ThinLTO 内核由 Makefile 自动加 `LLVM=1` |
| `_DSM` 返回 `0x80000002` | 该命令在本机固件不支持；查 `dmesg`  |
| 曲线写入后无效果 | 确认已切到 `custom` 模式（CLI/UI 自动；直接写 sysfs 需手动 `echo custom > fan_mode`） |
