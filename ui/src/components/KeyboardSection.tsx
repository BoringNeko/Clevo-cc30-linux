import { useEffect, useMemo, useState } from "react";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import FormControl from "@mui/material/FormControl";
import MenuItem from "@mui/material/MenuItem";
import Select from "@mui/material/Select";
import Slider from "@mui/material/Slider";
import Stack from "@mui/material/Stack";
import ToggleButton from "@mui/material/ToggleButton";
import ToggleButtonGroup from "@mui/material/ToggleButtonGroup";
import Typography from "@mui/material/Typography";
import KeyboardIcon from "@mui/icons-material/Keyboard";
import { ColorPicker } from "./ColorPicker";
import { SettingsRow } from "./SettingsRow";
import {
  getKeyboard,
  setKeyboardBrightness,
  setKeyboardKey,
  setKeyboardMode,
  setKeyboardZone,
  type KeyboardState,
} from "../api/daemon";
import { hexToRgbTuple } from "../lib/color";

const EMPTY_COLOR: [number, number, number] = [0, 0, 0];
const ZONES = ["all", "left", "middle", "right"] as const;
type Zone = (typeof ZONES)[number];

function colorToHex(color: number[] | undefined): string {
  if (!color || color.length !== 3) return "#000000";
  return `#${color.map((part) => part.toString(16).padStart(2, "0")).join("")}`;
}

function updateZone(keys: number[][][], zone: Zone, color: [number, number, number]) {
  const next = keys.map((row) => row.map((key) => [...key]));
  const columns = zone === "all" ? [0, 19] : zone === "left" ? [0, 6] : zone === "middle" ? [7, 12] : [13, 19];
  for (let row = 0; row < 6; row += 1) {
    for (let col = columns[0]; col <= columns[1]; col += 1) next[row][col] = [...color];
  }
  return next;
}

/** Keyboard RGB controls, hidden when the controller is absent. */
export function KeyboardSection({ open }: { open: boolean }) {
  const [state, setState] = useState<KeyboardState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [color, setColor] = useState("#00aaff");
  const [zone, setZone] = useState<Zone>("all");
  const [busy, setBusy] = useState(false);

  const refresh = async () => {
    try {
      setError(null);
      setState(await getKeyboard());
    } catch (err) {
      setError(String(err));
    }
  };

  useEffect(() => {
    if (open) void refresh();
  }, [open]);

  const rgb = useMemo(() => hexToRgbTuple(color) ?? EMPTY_COLOR, [color]);
  const disabled = !state?.available || !state.writable || busy;

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    try {
      await action();
      await refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  if (error && !state) {
    return <Typography sx={{ color: "text.secondary", fontSize: "0.8rem" }}>{error}</Typography>;
  }

  if (!state?.available) {
    const firmwareMessage = state?.firmware_kb_type === 6 || state?.firmware_kb_type === 22
      ? `固件报告键盘灯类型 ${state.firmware_kb_type}，但当前没有可验证的 Linux 写入通道。`
      : "未检测到可写的键盘灯控制器。";
    return (
      <Stack spacing={1.5} sx={{ py: 2 }}>
        <KeyboardIcon sx={{ color: "text.disabled", fontSize: 32 }} />
        <Typography sx={{ fontSize: "0.9rem", fontWeight: 600 }}>{firmwareMessage}</Typography>
        <Typography sx={{ fontSize: "0.78rem", color: "text.disabled" }}>
          支持的 ITE 829x USB 控制器（048d:8910）接入后，这里会显示亮度、灯效和单键颜色控制。
        </Typography>
      </Stack>
    );
  }

  const keys = state.keys.length === 6 ? state.keys : Array.from({ length: 6 }, () => Array.from({ length: 20 }, () => [0, 0, 0]));
  const isAcpiRgb15 = state.backend === "acpi-dchu";

  return (
    <Stack spacing={1.5}>
      <SettingsRow title="灯效" description={isAcpiRgb15 ? "ACPI-DCHU RGB15 单区" : "USB HID 键盘控制器"}>
        <FormControl size="small" sx={{ minWidth: 130 }}>
          <Select
            value={state.mode}
            onChange={(event) => {
              const mode = event.target.value as KeyboardState["mode"];
              void run(async () => setKeyboardMode(mode));
            }}
            disabled={disabled}
            aria-label="键盘灯效"
          >
            <MenuItem value="off">关闭</MenuItem>
            <MenuItem value="static">静态</MenuItem>
            <MenuItem value="wave">波浪</MenuItem>
          </Select>
        </FormControl>
      </SettingsRow>

      <SettingsRow title="亮度">
        <Slider
          value={state.brightness}
          min={0}
          max={4}
          step={1}
          disabled={disabled}
          onChangeCommitted={(_, value) => void run(async () => { await setKeyboardBrightness(value as number); })}
          aria-label="键盘亮度"
          sx={{ width: 150 }}
        />
      </SettingsRow>

      <SettingsRow title={isAcpiRgb15 ? "键盘颜色" : "分区颜色"} description={isAcpiRgb15 ? "整块键盘共用一个 RGB 通道" : "选择颜色后应用到全部、左侧、中部或右侧"}>
        <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
          <ColorPicker value={color} onChange={setColor} label="选择键盘颜色" />
          <ToggleButtonGroup
            exclusive
            size="small"
            value={zone}
            onChange={(_, value: Zone | null) => value && setZone(value)}
            disabled={disabled}
            aria-label="键盘分区"
          >
            {(isAcpiRgb15 ? ["all"] : ZONES).map((value) => <ToggleButton key={value} value={value}>{value === "all" ? "全部" : value === "left" ? "左" : value === "middle" ? "中" : "右"}</ToggleButton>)}
          </ToggleButtonGroup>
          <Button
            size="small"
            variant="outlined"
            disabled={disabled}
            onClick={() => void run(async () => {
              await setKeyboardZone(zone, rgb);
              setState((current) => current ? { ...current, keys: updateZone(current.keys, zone, rgb) } : current);
            })}
          >应用</Button>
        </Box>
      </SettingsRow>

      {!isAcpiRgb15 ? <Box>
        <Typography sx={{ fontSize: "0.8125rem", fontWeight: 600, mb: 1 }}>单键颜色</Typography>
        <Box sx={{ overflowX: "auto", pb: 1 }}>
          <Box sx={{ display: "grid", gridTemplateColumns: "repeat(20, 22px)", gap: "4px", minWidth: 516 }}>
            {keys.flatMap((row, rowIndex) => row.map((key, colIndex) => (
              <Box
                component="button"
                type="button"
                key={`${rowIndex}-${colIndex}`}
                title={`第 ${rowIndex + 1} 行，第 ${colIndex + 1} 列`}
                aria-label={`设置第 ${rowIndex + 1} 行第 ${colIndex + 1} 列键颜色`}
                disabled={disabled}
                onClick={() => void run(async () => {
                  await setKeyboardKey(rowIndex, colIndex, rgb);
                  setState((current) => {
                    if (!current) return current;
                    const next = current.keys.map((currentRow) => currentRow.map((currentKey) => [...currentKey]));
                    next[rowIndex][colIndex] = [...rgb];
                    return { ...current, keys: next };
                  });
                })}
                sx={{ width: 22, height: 22, p: 0, borderRadius: 0.5, border: "1px solid", borderColor: "divider", backgroundColor: colorToHex(key), cursor: disabled ? "default" : "pointer", opacity: disabled ? 0.55 : 1 }}
              />
            ))) }
          </Box>
        </Box>
      </Box> : <Typography sx={{ fontSize: "0.78rem", color: "text.secondary" }}>
        当前键盘为单区 RGB15，所有按键共用同一个颜色通道。
      </Typography>}
    </Stack>
  );
}
