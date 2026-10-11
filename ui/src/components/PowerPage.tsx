import { useCallback, useEffect, useState } from "react";
import Alert from "@mui/material/Alert";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Chip from "@mui/material/Chip";
import IconButton from "@mui/material/IconButton";
import LinearProgress from "@mui/material/LinearProgress";
import MenuItem from "@mui/material/MenuItem";
import FormControlLabel from "@mui/material/FormControlLabel";
import Switch from "@mui/material/Switch";
import TextField from "@mui/material/TextField";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";
import BatteryChargingFullIcon from "@mui/icons-material/BatteryChargingFull";
import PowerSettingsNewIcon from "@mui/icons-material/PowerSettingsNew";
import RefreshIcon from "@mui/icons-material/Refresh";
import SaveIcon from "@mui/icons-material/Save";
import { DESKTOP_PROFILES, getPower, setChargeThresholds, type Battery, type PowerSnapshot } from "../api/power";
import { setPerfMode } from "../api/daemon";
import { CardHeader, GlassCard } from "./GlassCard";

const STATUS: Record<string, string> = { Charging: "充电中", Discharging: "放电中", Full: "已充满", "Not charging": "未充电", Unknown: "状态未知" };

function BatteryItem({ battery, refresh }: { battery: Battery; refresh: () => Promise<void> }) {
  const [start, setStart] = useState(String(battery.thresholds?.start ?? ""));
  const [end, setEnd] = useState(String(battery.thresholds?.end ?? ""));
  const [enabled, setEnabled] = useState(battery.thresholds?.enabled ?? true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const supported = battery.end_supported && battery.thresholds !== null
    && (!battery.start_supported || battery.thresholds.start !== null);
  const startValue = battery.start_supported ? Number(start) : null;
  const endValue = Number(end);
  const native = battery.charge_control;
  const valid = end.trim() !== "" && Number.isInteger(endValue) && endValue > 0 && endValue <= 100
    && (startValue === null || (start.trim() !== "" && Number.isInteger(startValue) && startValue >= 0 && startValue < endValue));
  const allowed = !native || (startValue !== null && native.start_values.includes(startValue) && native.end_values.includes(endValue));
  const changed = (battery.start_supported && startValue !== battery.thresholds?.start) || endValue !== battery.thresholds?.end || (native && enabled !== battery.thresholds?.enabled);
  const apply = async () => {
    setBusy(true);
    setError(null);
    try {
      if (native) await setChargeThresholds(battery.name, startValue, endValue, enabled);
      else await setChargeThresholds(battery.name, startValue, endValue);
      await refresh();
    } catch (err) { setError(String(err)); }
    finally { setBusy(false); }
  };
  return (
    <GlassCard sx={{ gap: 2 }}>
      <CardHeader icon={<BatteryChargingFullIcon sx={{ fontSize: 18 }} />} title={`电池 ${battery.name}`} />
      {!battery.present ? <Typography color="text.secondary">电池未连接</Typography> : <>
        <Box sx={{ display: "flex", alignItems: "baseline", justifyContent: "space-between", gap: 2 }}>
          <Typography sx={{ fontSize: "2rem", fontWeight: 600 }}>{battery.capacity === null ? "未知" : `${battery.capacity}%`}</Typography>
          <Typography color="text.secondary">{STATUS[battery.status ?? "Unknown"] ?? battery.status}</Typography>
        </Box>
        {battery.capacity !== null && <LinearProgress aria-label="电池电量" variant="determinate" value={battery.capacity} sx={{ height: 6, borderRadius: 1 }} />}
        <Box sx={{ display: "flex", gap: 3, flexWrap: "wrap", color: "text.secondary" }}>
          <Typography variant="body2">健康度 {battery.health_percent === null ? (battery.health === "Good" ? "正常" : battery.health ?? "未知") : `${battery.health_percent.toFixed(1)}%`}</Typography>
          <Typography variant="body2">循环次数 {battery.cycle_count ?? "未知"}</Typography>
        </Box>
        {native && <FormControlLabel control={<Switch checked={enabled} disabled={!supported || busy} onChange={(_event, value) => setEnabled(value)} />} label="充电阈值控制" />}
        <Box sx={{ display: "flex", alignItems: "center", gap: 2, pt: 1, flexWrap: "wrap" }}>
          <TextField select={!!native} label="开始充电 (%)" type={native ? undefined : "number"} size="small" value={battery.start_supported ? start : ""} disabled={!supported || !battery.start_supported || busy || (!!native && !enabled)} onChange={(event) => setStart(event.target.value)} slotProps={{ htmlInput: { min: 0, max: 99 } }} sx={{ width: 170 }}>
            {native?.start_values.map((value) => <MenuItem key={value} value={String(value)}>{value}%</MenuItem>)}
          </TextField>
          <TextField select={!!native} label="停止充电 (%)" type={native ? undefined : "number"} size="small" value={battery.end_supported ? end : ""} disabled={!supported || busy || (!!native && !enabled)} onChange={(event) => setEnd(event.target.value)} slotProps={{ htmlInput: { min: 1, max: 100 } }} sx={{ width: 170 }}>
            {native?.end_values.map((value) => <MenuItem key={value} value={String(value)}>{value}%</MenuItem>)}
          </TextField>
          <Button startIcon={<SaveIcon />} variant="outlined" disabled={!supported || !valid || !allowed || !changed || busy} onClick={() => void apply()}>保存</Button>
        </Box>
        {!supported && <Typography variant="body2" color="text.secondary">{battery.end_supported ? "充电阈值暂时无法读取" : "当前 Linux 驱动未提供充电阈值接口"}</Typography>}
        {supported && !valid && <Alert severity="warning">开始阈值必须低于停止阈值，范围为 0–100%。</Alert>}
      </>}
      {battery.errors.length > 0 && <Alert severity="warning">部分电池信息不可用：{battery.errors.join("; ")}</Alert>}
      {error && <Alert severity="error">{error}</Alert>}
    </GlassCard>
  );
}

export function PowerPage({ onModeChanged }: { onModeChanged: () => void }) {
  const [snapshot, setSnapshot] = useState<PowerSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const refresh = useCallback(async () => {
    const value = await getPower();
    setSnapshot(value);
    setError(null);
  }, []);
  useEffect(() => {
    let active = true;
    let reading = false;
    const read = async () => {
      if (reading) return;
      reading = true;
      try {
        const value = await getPower();
        if (active) { setSnapshot(value); setError(null); }
      } catch (err) { if (active) setError(String(err)); }
      finally { reading = false; }
    };
    void read();
    const timer = window.setInterval(() => void read(), 5000);
    return () => { active = false; window.clearInterval(timer); };
  }, []);
  const profiles = DESKTOP_PROFILES.filter((entry) => snapshot?.platform_profile.driver === "clevo-cc" && snapshot.platform_profile.choices.includes(entry.profile));
  const changeMode = async (profile: string) => {
    const choice = profiles.find((entry) => entry.profile === profile);
    if (!choice) return;
    setBusy(true);
    setError(null);
    try { await setPerfMode(choice.mode); await refresh(); onModeChanged(); }
    catch (err) { setError(String(err)); }
    finally { setBusy(false); }
  };
  return (
    <Box sx={{ display: "grid", gridTemplateColumns: "repeat(2, minmax(0, 1fr))", gridTemplateRows: "max-content", alignItems: "start", gap: 2, overflowY: "auto", minHeight: 0, flex: 1, pb: 1, overflowWrap: "anywhere" }}>
      <GlassCard sx={{ gap: 2 }}>
        <CardHeader icon={<PowerSettingsNewIcon sx={{ fontSize: 18 }} />} title="桌面电源模式" />
        <Box sx={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 2 }}>
          <Chip label={snapshot?.ac_online === true ? "外接电源" : snapshot?.ac_online === false ? "电池供电" : "电源状态未知"} variant="outlined" />
          <Tooltip title="刷新电源信息"><IconButton aria-label="刷新电源信息" onClick={() => void refresh().catch((err) => setError(String(err)))}><RefreshIcon /></IconButton></Tooltip>
        </Box>
        {error && <Alert severity="error">无法读取电源信息：{error}</Alert>}
        {!snapshot && !error && <Typography color="text.secondary">加载中…</Typography>}
        {snapshot && !snapshot.available && <Alert severity="info">当前后端未提供电源数据</Alert>}
        {snapshot?.available && <>
          {profiles.length > 0 ? <TextField select label="电源模式" value={profiles.some((entry) => entry.profile === snapshot.platform_profile.current) ? snapshot.platform_profile.current : ""} disabled={busy} onChange={(event) => void changeMode(event.target.value)} size="small" sx={{ maxWidth: 320 }}>
            <MenuItem value="" disabled>当前模式未知</MenuItem>
            {profiles.map((entry) => <MenuItem key={entry.profile} value={entry.profile}>{entry.label}</MenuItem>)}
          </TextField> : <Typography variant="body2" color="text.secondary">{snapshot.platform_profile.choices.length ? "桌面电源模式由其他驱动管理" : "桌面电源模式接口不可用"}</Typography>}
          {snapshot.errors.length > 0 && <Alert severity="warning">{snapshot.errors.join("; ")}</Alert>}
        </>}
      </GlassCard>
      <Box sx={{ display: "flex", flexDirection: "column", gap: 2, minWidth: 0 }}>
        {snapshot?.available && <>
          {snapshot.batteries.length === 0 && <Typography color="text.secondary">未检测到电池</Typography>}
          {snapshot.batteries.map((battery) => <BatteryItem key={`${battery.name}:${battery.thresholds?.enabled}:${battery.thresholds?.start}:${battery.thresholds?.end}:${battery.charge_control?.backend}`} battery={battery} refresh={refresh} />)}
        </>}
      </Box>
    </Box>
  );
}
