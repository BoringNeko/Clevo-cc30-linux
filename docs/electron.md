# Electron 打包（与 Tauri 2 共存）

桌面 UI 现在有两个可选的壳，**共用同一套 React 前端与同一个 Rust 后端 crate**：

| 壳 | 窗口/托盘 | 前端如何调后端 | 安装 |
|---|---|---|---|
| **Tauri 2**（默认，`ui/`） | 进程内 Wry / WebKitGTK | Tauri IPC (`invoke`) | `install.sh --ui` |
| **Electron**（新增） | Chromium + Node 主进程 | 本地 HTTP 桥 | `install.sh --electron` |

**为什么加 Electron**：WebKitGTK 在 NVIDIA 专有驱动上有一串问题——启动即崩的
`Gdk Error 71`（[`hardware-notes.md`](hardware-notes.md) §15.1）与退出时的 EGL
段错误（§15.4）。两者都在 WebKitGTK 内部，Tauri 侧只能靠环境变量绕过。Electron
自带 Chromium，**根本不经过 WebKitGTK**，所以在这些机器上更稳。

两个壳都不接触硬件：都把命令发给 `clevod`（D-Bus / PolicyKit）。差别只在「前端
如何到达那层后端命令」。默认仍是 Tauri 2，Electron 是纯新增，不改变任何现有行为。

---

## 1. 共存模型

```
                    ui/src/  ← 同一套 React 前端（只有一份）
                   ╱        ╲
       Tauri 2 壳             Electron 壳
    （内嵌窗口 / 托盘）      （Chromium 窗口 / 托盘）
          │                        │
     Tauri IPC                本地 HTTP 桥
          │                        │
          └───────────┬────────────┘
                      ▼
        ui/src-tauri/  ← 同一个 Rust crate（命令逻辑只有一份）
         ├─ feature `tauri-shell`（默认）→ 带 Tauri 壳的二进制
         └─ --no-default-features        → 无壳后端（`--serve`）
                      │
                      ▼
          org.clevo.CC ▸ clevod ▸ 内核驱动 / acpi_call ▸ EC
```

**共享**：前端（`ui/src`）、后端命令逻辑（`ui/src-tauri/src/commands.rs`）、
协议库（`clevo-proto`）、守护进程（`clevod`）。

**各自独立**（外壳必然的重复，行为已对齐）：
- Tauri：`ui/src-tauri/src/tray.rs` + `lib.rs::run()`
- Electron：`ui/electron/main.cjs`（窗口、托盘、性能模式菜单）

两者都实现相同的外观与交互：自绘标题栏、关闭到托盘、托盘里的性能模式菜单
（`▶`/`・` 标记当前项）、`customize` 风扇曲线模式。

产物与文件名：

| | Tauri 2 | Electron |
|---|---|---|
| 二进制 | `target/release/clevo-cc-ui` | `target/electron/release/clevo-cc-ui` + Electron 运行时 |
| 可执行名 | `clevo-cc-ui` | `clevo-cc-ui-electron` |
| 桌面文件 | `org.clevo.cc.ui.desktop` | `org.clevo.cc.ui.electron.desktop` |
| AppImage | `clevo-cc-ui-<ver>-x86_64.AppImage` | `clevo-cc-ui-electron-<ver>-x86_64.AppImage` |

**两者可同时安装**（`sudo packaging/install.sh --ui --electron`），共存于同一个
`clevod`，`uninstall.sh` 会一并清理。

---

## 2. 前端如何同时适配两个壳

`ui/src/api/bridge.ts` 是唯一的适配点：

| 函数 | 作用 |
|---|---|
| `invokeBridge(cmd, args)` | 有 `window.__CLEVO_ELECTRON__` 用它，否则退回 Tauri `invoke` |
| `windowBridge()` | 标题栏窗口控制（最小化 / 全屏 / 关闭 / 改尺寸 / 监听 resize） |
| `quitBridge()` | 退出整个应用 |

所有 hook 与组件都改走它（`daemon.ts`、`useWallpaper/Logo/AppSettings/WindowSize`、
`WindowControls`）。**用特性探测而不是 UA 判断**，所以 `vite dev`（两个壳都没有）
仍能预览，只是写操作落到已有的「非壳环境」回退分支。

前端资源必须用**相对路径**：`vite.config.ts` 设 `base: "./"`。Vite 默认的
`base: "/"` 产出 `/assets/...`，在 Electron 的 `file://` 下会解析到文件系统根，
所有脚本 404、窗口白屏（进程还在，只看日志发现不了）。Tauri 用自定义协议所以
不受影响，`./` 对两者都安全。

---

## 3. 后端桥（`--serve`）

同一个 Rust crate 用 `--no-default-features` 编成**无 Tauri 壳**的可执行文件，
多出一个 `--serve` 模式（`ui/src-tauri/src/serve.rs`）：监听 `127.0.0.1` 随机端口，
把前端的 `invoke` 转成对命令函数的调用。这样 D-Bus / PolicyKit / 壁纸 / logo 的
逻辑只有一份，Rust 测试继续覆盖它。

**协议**
- 启动时向 stdout 打印一行握手：`CLEVO_CC_BACKEND <port> <token>`。
- `POST /invoke`，body `{"command": "...", "args": {...}}`，带 header
  `x-clevo-token: <token>`；回复 `{"ok":true,"value":...}` 或
  `{"ok":false,"error":"..."}`。

**防护**
- 只绑定 `127.0.0.1`，端口随机。
- token 每次运行随机；缺 token / 错 token → `403`。
- 未知命令 / 缺参数返回 `{"ok":false,...}`，不会 panic。
- CORS 仅放行 `Origin: null`（`file://` 生产）与 `http://localhost:*` /
  `http://127.0.0.1:*`（开发）。

**Arg3 形状**由 DSDT 决定，桥沿用与 Tauri 完全相同的行为（见
[`hardware-notes.md`](hardware-notes.md) §3、§13）。桥不新增任何硬件语义。

---

## 4. 构建

前置：Node ≥ 18、pnpm、Rust。

```bash
cd ui
pnpm install            # 下载 Electron 运行时（~100 MB，首次慢）
pnpm build              # 构建前端 dist/
pnpm electron           # 用已下载的 Electron 直接跑（开发）
```

打包发行产物：

```bash
pnpm electron:build                  # AppImage / deb / rpm → ui/release/
packaging/build-packages.sh electron # 同上，并复制到 dist/
```

`install.sh --electron` 还需要**无壳后端**，其构建命令是：

```bash
cd ui/src-tauri
CARGO_TARGET_DIR=target/electron cargo build --release --locked --no-default-features
```

electron-builder 通过 `extraResources` 把 `src-tauri/target/release/clevo-cc-ui`
打进 AppImage；`build-packages.sh` / `install.sh` 会先把无壳后端复制到该路径。

### 4.1 下载 Electron 失败怎么办

`pnpm install` 会从 GitHub Releases 下载 Electron 运行时（`objects.githubusercontent.com`），
网络受限时 `ETIMEDOUT`。可换镜像：

```bash
ELECTRON_MIRROR=https://npmmirror.com/mirrors/electron/ pnpm install
# 若仍失败，可手动下载并解压：
#   https://cdn.npmmirror.com/binaries/electron/<ver>/electron-v<ver>-linux-x64.zip
#   解压到 ui/node_modules/electron/dist/ ，并写 path.txt 内容为 “electron”
```

这是环境网络问题，不影响代码。

---

## 5. 安装

```bash
sudo packaging/install.sh --electron            # 装 Electron 版 UI
sudo packaging/install.sh --electron --enable   # 顺带启用并启动 clevod
sudo packaging/install.sh --ui                  # 装 Tauri 2 版（原行为）
sudo packaging/install.sh --ui --electron       # 两个都装
sudo packaging/uninstall.sh                     # 全部回滚
```

Electron 版安装到：
- `/usr/bin/clevo-cc-ui-electron`（启动器）
- `/usr/lib/clevo-cc-ui-electron/`（Electron 应用树）
- `/usr/share/applications/org.clevo.cc.ui.electron.desktop`

---

## 6. 关窗到托盘的行为（方案 2）

Electron 默认的「关窗」若只做 `win.hide()`，**整个 Chromium 进程树
（renderer / GPU / utility）会一直常驻**——实测一个隐藏窗口仍占约 1.3 GB。
本实现改为**关闭即销毁窗口**，只保留托盘与后端：

| 阶段 | renderer 进程 | Electron RSS |
|---|---|---|
| 窗口打开 | 1 | ~1250 MB |
| 关闭到托盘 | **0** | **~600 MB** |
| 托盘重新打开 | 1 | 回到 ~1250 MB |

机制（`ui/electron/main.cjs`）：
- 关闭按钮 → `window.__CLEVO_ELECTRON__.window.hide()` → 主进程 `destroyWindow()`
  （先移除 `close` 监听再 `destroy()`，避免递归）。
- **托盘保留**：菜单「打开控制中心」与托盘左键都调用 `showWindow()`；窗口已销毁
  时 `createWindow()` 重建（幂等）。
- **后端不重启**：`clevo-cc-ui --serve` 继续运行，保持 D-Bus 连接与缓存；重建的
  窗口在正常启动流程里重新拉快照。
- 代价：重开有一次前端重新渲染的延迟（远小于 600 MB 常驻开销）。

与 Tauri 的关系：Tauri 关窗后 WebKit 的 web process 本来就会被销毁（也因此才有
§15.4 的退出崩溃），所以两边「关窗后不常驻浏览器进程」行为一致；差别只是 Electron
需要显式 `destroy()`。

---

## 7. 兼容性设置

- **显示后端（Wayland/X11）**：Electron 下由 Chromium 按会话选择，设置里该项对
  Electron 生效方式与 Tauri 不同（对 Tauri 照旧）。
- **软件渲染**：打开后 `launch_env` 写 `CLEVO_CC_SOFTWARE_RENDERING=1`，Electron
  主进程读到后加 `--disable-gpu` 等开关。给其他坏驱动的兜底。
- WebKit 专用变量（`WEBKIT_DMABUF_RENDERER_FORCE_SHM`、`WEBKIT_SKIA_ENABLE_CPU_RENDERING`）
  在 Electron 下**无关**。`launch_env` 按壳只设置各自需要的变量（见
  `ui/src-tauri/src/launch_env.rs`）。

---

## 8. 如何测试

测试分四层，从完全离线到需真机 + 显示器。命令在**仓库根**执行。

### 8.1 一键全套（每次改动都跑）

```bash
scripts/run-tests.sh          # 全套（含下列所有 Electron 检查）
scripts/run-tests.sh --ui     # 只跑 UI 相关
```

需要 `dbus-run-session`（D-Bus 集成测试）、Node + pnpm；Electron 运行时与显示器
为可选项，缺失时相关脚本自动跳过。

Electron 相关步骤：

| 步骤 | 验证什么 |
|---|---|
| `node --check ui/electron/*.cjs` | 主进程 / preload 语法 |
| `scripts/tests-electron-preload.mjs` | preload 暴露面与 `bridge.ts` 一致 |
| `scripts/tests-electron-bridge.sh` | 跨语言契约：握手串、`/invoke`、token、资源相对路径、后端查找路径 |
| `cargo test --no-default-features` | 无壳后端构建 + `--serve` 真打 HTTP |
| `scripts/electron-e2e.sh` | 后端握手 + 桥接 +（有显示器时）真实启动 Electron |
| `scripts/electron-tray-release.sh` | 关窗释放 renderer、托盘/后端存活、可重建 |

### 8.2 手动分层验证

**第 1 层：后端桥（不启动 Electron）**

```bash
cd ui/src-tauri && CARGO_TARGET_DIR=target/electron cargo build --release --no-default-features && cd ../..
./ui/src-tauri/target/electron/release/clevo-cc-ui --serve
# 打印: CLEVO_CC_BACKEND <port> <token>
curl -s -X POST -H "x-clevo-token: <token>" \
  http://127.0.0.1:<port>/invoke -d '{"command":"get_fan_snapshot","args":{}}'
```

无 / 错 token 应 403。

**第 2 层：Electron 启动（需显示器）**

```bash
scripts/electron-e2e.sh
```

**第 3 层：确认界面真的渲染了（推荐）**

Electron 主进程不会把渲染错误打到终端，用 Chromium 调试协议最直观：

```bash
cd ui
./node_modules/electron/dist/electron . --no-sandbox --remote-debugging-port=9222
# 另开终端：
curl -s http://127.0.0.1:9222/json | python3 -m json.tool
```

再连 CDP 取 DOM 文本 / 截图，可确认卡片、实时转速、强调色都出来了。本仓库验证时
看到真实硬件数据（如 `CPU · 2013 RPM`、`CPU 42° · GPU 37°`）。

**第 4 层：真机 + 真守护进程**

`clevod` 在系统总线上跑着时，Electron 版直接连它：

```bash
systemctl status clevod
cd ui && ./node_modules/electron/dist/electron . --no-sandbox
```

界面读数应与 `clevo-cc fan status` 一致。

### 8.3 关窗释放的回归测试

```bash
scripts/electron-tray-release.sh
```

真实启动 → 驱动 UI 关闭路径 → 断言 renderer 归零且 RSS 下降 → 后端存活 →
窗口可重建。它用测试专用环境变量 `CLEVO_CC_E2E_REOPEN_MS` 触发重开（生产默认关闭）。

### 8.4 两个只在真机暴露的坑（已修，有回归测试）

1. **前端资源必须相对路径**：`base: "./"`，否则 `file://` 下白屏。
2. **后端二进制查找路径**：`main.cjs` 用 `__dirname` 相对查找，无壳产物在
   `target/electron/release/`（不是 `target/release/`）。

---

## 9. 已知差异小结

| 项 | Tauri 2 | Electron |
|---|---|---|
| 渲染引擎 | WebKitGTK | Chromium |
| 产物体积 | 小（~10 MB） | 大（~150 MB+） |
| 托盘 | `muda`/AppIndicator | Electron `Tray` |
| 下载/构建 | 快 | 慢（下载 Electron） |
| NVIDIA 启动/退出问题 | 需环境变量绕过 | 不涉及 |
| 关窗后内存 | WebKit 进程销毁 | 显式 `destroy()` 释放（§6） |
| 重新打开 | 重建 webview | 重建窗口（§6） |
