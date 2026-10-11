# Doctor 诊断

`doctor` 检查安装环境、硬件读取、服务、权限和配置，逐项输出结论与修复建议。
某项失败不会中断后续检查。它不自动修复、不启动服务、不写入硬件设置、不修改配置。

## 使用

推荐通过正在运行的守护进程检查：

```bash
cargo run -p clevo-cc-cli -- --transport dbus doctor
# 已安装 CLI 时：
clevo-cc --transport dbus doctor
```

也可检查直接访问驱动或 acpi_call 的路径：

```bash
clevo-cc --transport driver doctor
sudo clevo-cc --transport acpi-call doctor
```

默认 transport 是 `mock`，因此单独运行 `clevo-cc doctor` 只检查离线 fixture，
不探测宿主硬件、服务或配置。诊断真实机器时必须明确选择 transport。

```bash
clevo-cc --transport dbus doctor --json
clevo-cc --transport dbus doctor --config /path/to/clevod.toml
clevo-cc --transport dbus --dbus-session doctor
```

`--config` 默认是 `/etc/clevo-cc/clevod.toml`。守护进程使用自定义路径时，
请传入与其 `--config` 相同的路径。`--dbus-session` 用于会话总线上的测试服务。

## 检查内容

- 内核版本、`CONFIG_ACPI` 和匹配的内核头文件。
- DMI 厂商、产品与主板名称；CLV0001 的 `_STA` 状态。
- 模块、平台驱动绑定、模式属性和 hwmon 风扇输入。
- acpi_call 接口；直接访问时读取风扇状态、曲线和 AppSettings/GCMD 能力。
- RGB15 sysfs 与 ITE `048d:8910` hidraw 设备，读取探测信息而不打开 HID 控制器。
- 电池容量、状态、健康度、标准充电阈值，以及 platform_profile 接口。
- 当前进程的 UID/GID/组、有效访问权限及安装器 udev 规则。
- D-Bus 连接、服务名称所有者、守护进程缓存状态、能力及键盘可用性。
- systemd 单元、D-Bus/PolicyKit 文件，以及配置目录、TOML 类型和 schema 兼容性。

配置检查复用 `clevod` 的解析器；旧 schema 的迁移只发生在内存中。
权限检查使用有效进程身份并考虑 ACL，不尝试向 sysfs/hidraw 写入数据。
权限允许仅表示访问检查通过，不保证固件接受后续写入。
`writable API` 表示后端是否提供写接口，实际权限另列检查。

D-Bus 检查先查询现有名称所有者，并固定到该实例的唯一名称；服务不存在时直接报告，
不触发 D-Bus 自动激活。诊断连接的单次方法调用超时为 3 秒。
它不调用 `Poll`，所以缓存未知或陈旧时会警告；能力读取可能触发只读固件查询。
acpi_call 为发送只读查询会写入 `/proc/acpi/call` 的请求接口，且不会发送设置命令。

电源检查目前读取标准 `power_supply` 阈值节点和旧式
`/sys/firmware/acpi/platform_profile*`。它不检查原生 FlexiCharger，也不枚举新式
`/sys/class/platform-profile` provider；这些标准节点缺失而显示 `Skip`，不能据此
认定充电控制或桌面模式不可用。查看 daemon 实际发现的后端与当前状态：

```bash
clevo-cc --transport dbus power status --json
# 仅在该原生节点存在时：
cat /sys/devices/platform/CLV0001:00/flexicharger
```

原生档位、PolicyKit 拒绝、阈值验收及 UI/KDE 状态差异见 [power.md](power.md)。
doctor 不做写入测试，因此报告通过不等于已验证阈值的实际停止/恢复行为。

## 结果与退出码

| 状态 | 含义 |
|---|---|
| Pass | 检查通过 |
| Warn | 检查不完整或需要关注，例如内核配置不可读、缓存陈旧、缺少头文件 |
| Fail | 明确故障，例如所选服务无法连接、固件查询失败或配置无法解析 |
| Skip | 可选接口未提供或当前路径不需要，例如 RGB15 机器没有 ITE 控制器 |

没有 `Fail` 时退出码为 `0`，存在 `Fail` 时为 `1`；命令参数错误沿用 clap 的退出码。
警告与可选接口缺失不会单独导致失败。无法读取配置会警告，不会误判为 TOML 损坏。
不存在配置文件时按守护进程使用默认配置的行为处理。

JSON 报告包含 `report_version`、`transport`、`os`、`arch`、`changes_made=false`、
`checks` 和 `summary`；每条检查具有 `id`、`status`、`detail` 和可选的 `suggestion`。
即使服务连接失败，也会完整输出 JSON，便于脚本和问题报告使用。

## 与日志配合

Doctor 不收集完整运行日志。需要定位固件或服务错误时，可按建议查看：

```bash
systemctl status clevod
journalctl -u clevod -b --no-pager
journalctl -k -b --no-pager
```

诊断报告会包含机型、接口路径和错误详情；分享前可检查这些信息。
