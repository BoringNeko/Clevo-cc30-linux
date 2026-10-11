# 电池与桌面电源

UI 的「电源与电池」页面和 `clevo-cc power` 使用同一组 daemon 接口。
电池数据来自标准 `/sys/class/power_supply`；没有读数时显示未知，不把缺失值当成零。
页面每五秒更新，不依赖风扇读取成功。
左侧卡片包含外接电源提示、刷新按钮和桌面电源模式；右侧为各电池的状态与阈值。

## 当前验证结果

截至 2026-10-11，COLORFUL P15 23 已确认：

| 项目 | 结果 |
|---|---|
| 电池读取 | BAT0 电量、状态、容量健康度、循环次数及外接电源可读 |
| 标准充电阈值 | 本机没有标准 start/end threshold 节点 |
| 原生 FlexiCharger | 已通过 D-Bus 设置并回读 `enabled=1/start=40/end=80` |
| 当前充电状态 | 99% 时内核 `Not charging`、电流 0，UPower `fully-charged`、功率 0 W |
| 内核桌面模式 | `clevo-cc` provider 可读，当前 performance，上报四种 profile |
| 待验收 | 从低电量充到停止阈值、跌破开始阈值后的恢复、重启重放、KDE/GNOME 双向切换 |

固件阈值读回是固件变量，不是独立的 EC 应答。当前无充电电流不能替代一次完整的
充电周期验收；其他机型也需要单独验证。

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

省略 `--start` 会保留当前开始值，因此它仍须小于新的停止值。例如本机初始为
95–100%，不能直接把停止值改成 80%；应同时指定 `--start 40 --end 80`。

写入只经 D-Bus，PolicyKit action 为 `org.clevo.CC.set-charge-thresholds`。
daemon 在任何写入前验证 `0 <= start < end <= 100`、电池是否存在和接口是否可读。
调整窗口时选择保持中间状态有效的写入顺序，随后读回核对；失败会尝试恢复旧值，
恢复失败会明确报告；若读回确认旧状态完整保留，则报告旧状态已确认，不再误报
恢复失败。成功后保存到 schema 7 的 TOML，`apply_on_start = true`
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

未加载新驱动时显示「当前 Linux 驱动未提供充电阈值接口」。原生控制验证和写入
失败排查见下文；协议细节与实测记录见 [hardware-notes.md](hardware-notes.md) §17。

## UI 与 KDE 状态差异

本项目 UI 显示标准 `power_supply/status`；KDE 经 UPower 展示电池状态，两者
文字可能不同。本机启用窗口后显示如下，均未报告当前正在充电：

| 来源 | 实测 |
|---|---|
| 内核 / UI | `Not charging` / 「未充电」 |
| UPower | `fully-charged`，`energy-rate: 0 W` |
| KDE 弹窗 | 「完全充满 99%」 |

KDE 的外接电源图标可以带充电标记，不能仅凭图标判断有充电电流。
设置停止阈值不会把已有的 99% 电量主动降到 80%。

```bash
cat /sys/class/power_supply/BAT0/status
cat /sys/class/power_supply/BAT0/current_now
clevo-cc --transport dbus power status --json
upower --dump
```

状态、功率和电量趋势需要结合判断；不要把读数缺失当成 0，也不要以某一次功率
读数代替完整的充电周期测试。

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
clevo-cc --transport dbus profile set 2 --apply
clevo-cc --transport dbus power status --json
# 新式内核可查看各 provider；name=clevo-cc 的条目属于本项目。
for provider in /sys/class/platform-profile/*; do
  [ -d "$provider" ] || continue
  cat "$provider/name" "$provider/choices" "$provider/profile"
done
# 旧式接口存在时，可读取 /sys/firmware/acpi/platform_profile{,_choices}。
# 在 KDE/GNOME 电源设置切换模式后，再检查上述状态和持久化值。
grep perf_mode /etc/clevo-cc/clevod.toml
```

桌面具体显示哪些档位取决于其电源服务和内核 provider；可用时用
`powerprofilesctl list` / `powerprofilesctl get` 辅助检查，不强行同时启动多个电源管理器。
本机已读到 provider 的四种 choices 和 performance 状态；桌面双向联动仍待验收。
KDE 提示「电源管理方案不可用」并建议安装 `power-profiles-daemon`，表示桌面的
电源服务尚不可用，不等于 `clevo-cc` 的内核 provider 或充电阈值控制失效。先检查
发行版配置的电源服务，再测试桌面切换：

```bash
systemctl status power-profiles-daemon --no-pager
powerprofilesctl list
powerprofilesctl get
```

未安装工具或使用其他电源管理器时，按发行版的实际配置处理；无需为读取本项目
UI 或使用 CLI 阈值控制额外启用该服务。

## 阈值真机验收

先记录原来的 enabled/start/end 和配置文件内容。测试会改变充电策略，完成后
通过 D-Bus 恢复记录的原值，避免 daemon 下次启动又重放测试配置。

1. 读取 `flexicharger` 和 `power status --json`，确认原生电池绑定与允许档位。
2. 预览并应用 `--start 40 --end 80 --enabled true`；核对 sysfs、D-Bus 和 UI 为 40/80 且启用。
3. 在低于开始阈值时接电，观察充电过程是否在约停止阈值停止；再验证低于开始阈值时恢复。
4. 重启后核对启用状态和窗口，并检查 `journalctl -u clevod -b --no-pager` 是否有重放失败。
5. 用 `--enabled false` 验证关闭；核对读回并观察充电状态，最后恢复原配置。

每次读取状态不会写入硬件。单次配置成功只说明设置与回读匹配，不能跳过第 3 步。

## 常见错误

| 错误或现象 | 检查方式 |
|---|---|
| `AccessDenied` | 完成桌面的 PolicyKit 授权；取消或未通过授权不会写入。需要时用 `doctor` 检查策略安装 |
| `NotSupported` / 没有阈值控件 | 检查驱动节点和 JSON 的 `charge_control`；标准字段缺失不证明硬件不支持原生通道 |
| `InvalidArgs` | 检查 `start < end`、百分比范围和固件允许档位，省略 start 时也要检查保留的值 |
| `Invalid argument (os error 22)` | 本机曾由旧 daemon 分段写 sysfs 引发，更新 daemon 并重启服务；回读确认当前配置 |
| 设置 80% 后仍是 99% | 阈值不会主动放电；观察实际充电状态与后续充电周期 |

只更新 daemon/CLI 时可保留已安装的新驱动与 UI：

```bash
cargo build --release -p clevod -p clevo-cc-cli
sudo packaging/install.sh --bin-dir target/release --no-driver --enable
```

`--no-driver` 不移除或重载已有驱动。初次接入原生通道时仍需按上文完整安装驱动。

## 离线验证与 API

以下测试使用模拟固件、隔离 sysfs 或私有 D-Bus，不验证真实电池的停止/恢复行为：

```bash
dbus-run-session -- cargo test --workspace
bash scripts/tests-kernel-gcmd.sh
bash scripts/tests-kernel-profile.sh
bash scripts/tests-kernel-charge.sh
pnpm --dir ui test
pnpm --dir ui build
dbus-run-session -- cargo test --manifest-path ui/src-tauri/Cargo.toml
dbus-run-session -- cargo test --manifest-path ui/src-tauri/Cargo.toml --no-default-features
```

覆盖档位校验、电池路径绑定、授权拒绝、读回不匹配、失败恢复、旧配置迁移和重放。
sysfs 回归测试按每次 write 独立解析命令，防止格式化分段写入导致 EINVAL。

`GetPower` 返回结构化 JSON；`SetChargeThresholds(battery, start, end)` 中 start=255
表示保持原开始阈值或无开始阈值。`SetChargeWindow(battery, start, end, enabled)`
同时提交原生启用状态，使用同一个 PolicyKit action；标准接口拒绝原生启用状态。
系统服务允许标准 power_supply 的真实 sysfs 设备
路径写入，否则 `ProtectKernelTunables` 会阻止已授权的阈值变更；对外写入口仍经 PolicyKit。
