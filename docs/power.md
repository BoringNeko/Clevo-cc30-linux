# 电池与桌面电源

UI 的「电源与电池」页面和 `clevo-cc power` 使用同一组 daemon 接口。
电池数据来自标准 `/sys/class/power_supply`；没有读数时显示未知，不把缺失值当成零。
页面每五秒更新，不依赖风扇读取成功。

## 电池和充电阈值

读取容量、状态、循环次数，以及内核提供的 health 字段。
健康百分比使用同单位的 `energy_full / energy_full_design` 或
`charge_full / charge_full_design`；缺少对应字段时保持未知。
支持多电池、无电池，以及只提供停止阈值的设备。

```bash
clevo-cc --transport dbus power status
clevo-cc --transport dbus power status --json
# 预览，不写入；先确认 status 中该电池的阈值可用。
clevo-cc --transport dbus power set-thresholds BAT0 --end 80
# 只设置停止阈值；已有开始阈值保持不变。
clevo-cc --transport dbus power set-thresholds BAT0 --end 80 --apply
# 仅适用于同时支持开始和停止阈值的电池。
clevo-cc --transport dbus power set-thresholds BAT0 --start 40 --end 80 --apply
```

写入只经 D-Bus，PolicyKit action 为 `org.clevo.CC.set-charge-thresholds`。
daemon 在任何写入前验证 `0 <= start < end <= 100`、电池是否存在和接口是否可读。
调整窗口时选择保持中间状态有效的写入顺序，随后读回核对；失败会尝试恢复旧值，
恢复失败会明确报告。成功后保存到 schema 7 的 TOML，`apply_on_start = true`
时在启动恢复；旧 schema 自动补充空的阈值配置。

```toml
[charge_thresholds.BAT0]
start = 40
end = 80
```

本机 COLORFUL P15 23 没有标准 start/end threshold 节点，但本机 DSDT 和原厂
FlexiCharger 页面确认存在 `_DSM` command 4 的 selector 30/31。新驱动提供
`/sys/devices/platform/CLV0001:00/flexicharger`，daemon 在标准停止阈值节点缺失时
尝试此接口。只有接口中的 ACPI 电池路径与 power_supply 的 `device/path` 或
`device/firmware_node/path` 匹配时才启用，其他电池不套用此通道。

读取 `4/30` 返回启用状态、当前窗口、固件允许的开始/停止档位。空回包（含 EC 不可用）
和无效字段不会被当成有效能力。本机 DSDT 的开始档位为 40/50/60/70/80/95，停止
档位为 60/70/80/90/100；UI 使用固件实际回包中的档位，提供启用开关。标准接口仍
使用百分比输入，不提供原生开关。

写入 `4/31` 一次提交 `enabled/start/end`，内核在锁内检查档位、写入、读回，失败
尝试恢复旧启用状态和窗口。daemon 再次核对回读并持久化；无原生开关参数的旧调用
设置原生阈值时默认启用，关闭时保留窗口。重放保留保存的 enabled 值。

```bash
# 安装新模块后先只读，确认 battery 路径和允许档位。
cat /sys/devices/platform/CLV0001:00/flexicharger
clevo-cc --transport dbus power status --json
# 显式启用并设置 40–80 窗口；去掉 --apply 可先预览。
clevo-cc --transport dbus power set-thresholds BAT0 --start 40 --end 80 --enabled true --apply
# 关闭阈值控制，保留窗口值。
clevo-cc --transport dbus power set-thresholds BAT0 --start 40 --end 80 --enabled false --apply
```

```toml
[charge_thresholds.BAT0]
enabled = true
start = 40
end = 80
```

固件读回反映 BAEE/BSCP/BTCP 的固件变量，不能单独证明 EC 实际停止充电。
代码已完成协议及隔离测试。2026-10-11 真机读取已确认电池绑定、允许档位和初始
关闭状态（95–100%）；第一次设置曾因 daemon 分段写入 sysfs 返回 EINVAL，固件
配置保持原值。已将原生命令和标准阈值改为先格式化再完整写入，并增加每次 write
独立解析的回归测试。修复后用户已成功通过 D-Bus 写入并回读 `enabled=1/start=40/end=80`。
随后只读核对时 BAT0 电量为 99%，内核状态为 `Not charging`、电流为 0，UPower
显示 `fully-charged`、功率为 0 W，KDE 显示「完全充满 99%」。KDE 外接电源图标
不代表当前有充电电流；本次结果确认原生读写及当前未充电状态，完整充电过程的
阈值停止/恢复与重启重放仍待验收。验收时先记录原配置，完成后恢复；电量高于
停止阈值不会主动放电。未加载新驱动时显示「当前 Linux 驱动未提供充电阈值接口」。
标准阈值的授权、读回、恢复和重放已有隔离 sysfs/D-Bus 测试，支持机型仍需真机验证。
2026-10-11 的只读实测可读到 BAT0 电量、状态、容量健康度和循环次数；电量为 100%、
状态 Full，`energy_full / energy_full_design` 的计算结果约 62.8%，阈值接口均缺失。

## 桌面模式

`clevo-cc` 注册内核 `platform_profile`，与原来的 `perf_mode` 共用经 GCMD
能力校验的 `121/25` 写入路径。映射如下：

| Linux profile | 原厂模式 | 121/25 值 |
|---|---|---|
| quiet | quiet | 0 |
| low-power | pwrsaving | 1 |
| balanced | entertainment | 3 |
| performance | performance | 2 |

CLI/UI 写入后内核发出 profile 通知；桌面经 profile 写入后 daemon 在下一轮轮询
同步当前模式并持久化，风扇读取失败也不会阻止同步。UI 只操作属于 `clevo-cc` 的
provider，其他驱动的 profile 不会被误当成 Clevo 接口。

新内核优先读取 `/sys/class/platform-profile/*/{name,profile,choices}`，旧内核
读取 `/sys/firmware/acpi/platform_profile*`；构建时检测头文件 API，兼顾发行版回移。
需启用 `CONFIG_ACPI_PLATFORM_PROFILE`。注册失败不影响风扇和键盘功能。

没有经过验证的固件当前模式查询，因此驱动首次加载且没有已保存模式时，当前模式
读回可能返回 `ENODATA`。注册本身不切换模式；先经 CLI 成功设置一次，或让 daemon
恢复已保存模式，之后桌面即可读回。状态表示此驱动最后一次成功写入，无法识别绕过
驱动的固件内部模式变化。

## 安装后验收

先完成构建和测试，再安装新驱动、daemon 与对应 UI：

```bash
dbus-run-session -- cargo test --workspace
bash scripts/tests-kernel-profile.sh
bash scripts/tests-kernel-charge.sh
cargo build --release -p clevod -p clevo-cc-cli
# Electron 用户；安装器构建 UI 并重新加载旧驱动、重启 daemon。
sudo packaging/install.sh --bin-dir target/release --enable --electron
# Tauri 用户用 --ui 替代 --electron。
```

```bash
cat /sys/firmware/acpi/platform_profile_choices
clevo-cc --transport dbus profile set 2 --apply
cat /sys/firmware/acpi/platform_profile
clevo-cc --transport dbus power status
# 在 KDE/GNOME 电源设置切换模式后，再检查上述状态和持久化值。
grep perf_mode /etc/clevo-cc/clevod.toml
```

桌面具体显示哪些档位取决于其电源服务和内核 provider；可用时用
`powerprofilesctl list` / `powerprofilesctl get` 辅助检查，不强行同时启动多个电源管理器。
当前代码完成了接口与离线验证，真机桌面联动应在加载新模块后验收。

`GetPower` 返回结构化 JSON；`SetChargeThresholds(battery, start, end)` 中 start=255
表示保持原开始阈值或无开始阈值。`SetChargeWindow(battery, start, end, enabled)`
同时提交原生启用状态，使用同一个 PolicyKit action；标准接口拒绝原生启用状态。
系统服务允许标准 power_supply 的真实 sysfs 设备
路径写入，否则 `ProtectKernelTunables` 会阻止已授权的阈值变更；对外写入口仍经 PolicyKit。
