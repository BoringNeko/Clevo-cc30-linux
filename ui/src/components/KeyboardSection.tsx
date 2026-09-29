import { useState } from "react";
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
  KEYBOARD_BRIGHTNESS_MAX,
  KEYBOARD_ZONES,
  setKeyboardBrightness,
  setKeyboardMode,
  setKeyboardZone,
  type KeyboardMode,
  type KeyboardZone,
} from "../api/daemon";
import {
  isSingleZone,
  keyboardModeLabel,
  keyboardSupportedModes,
  keyboardUnavailableMessage,
  keyboardWritable,
  keyboardZoneLabel,
  keysColorToHex,
  normaliseKeys,
  type KeyboardKeys,
} from "../lib/keyboard";
import { useKeyboard } from "../hooks/useKeyboard";
import { hexToRgbTuple } from "../lib/color";

/**
 * Compact keyboard-backlight controls for the settings dialog.
 *
 * The sidebar's RGB page is the full editor; this section is the quick-access
 * version that lives with the rest of the settings. Both share the same hook and
 * helper logic, so they cannot disagree about the controller's capabilities.
 */
export function KeyboardSection({ open }: { open: boolean }) {
  const { state, error, busy, run } = useKeyboard(open);
  const [color, setColor] = useState<string | null>(null);
  const writable = keyboardWritable(state);
  const disabled = !writable || busy;
  const singleZone = state ? isSingleZone(state) : true;
  const modes = keyboardSupportedModes(state);
  const keys: KeyboardKeys = normaliseKeys(state?.keys);
  // The last colour the hardware shows, used until the user picks another one.
  const colorsOnKeyboard = (() => {
    for (const row of keys) {
      for (const entry of row) {
        if (entry[0] || entry[1] || entry[2]) return keysColorToHex(entry);
      }
    }
    return "#00aaff";
  })();
  const selectedColor = color ?? colorsOnKeyboard;

  if (error && !state) {
    return <Typography sx={{ color: "text.secondary", fontSize: "0.8rem" }}>{error}</Typography>;
  }

  if (!state?.available) {
    return (
      <Stack spacing={1.5} sx={{ py: 2 }}>
        <KeyboardIcon sx={{ color: "text.disabled", fontSize: 32 }} />
        <Typography sx={{ fontSize: "0.9rem", fontWeight: 600 }}>
          {keyboardUnavailableMessage(state)}
        </Typography>
        <Typography sx={{ fontSize: "0.78rem", color: "text.disabled" }}>
          支持的 ITE 829x USB 控制器（048d:8910）接入后，这里会显示亮度、灯效和颜色控制；
          侧栏的 RGB 页面提供完整的颜色编辑。
        </Typography>
      </Stack>
    );
  }

  const applyZoneColor = (zone: KeyboardZone) => {
    const rgb = hexToRgbTuple(selectedColor) ?? [0, 0, 0];
    void run(() => setKeyboardZone(zone, rgb as [number, number, number]));
  };

  return (
    <Stack spacing={1.5}>
      <SettingsRow
        title="灯效"
        description={singleZone ? "ACPI-DCHU RGB15 单区" : "USB HID 键盘控制器"}
      >
        <FormControl size="small" sx={{ minWidth: 130 }}>
          <Select
            value={state.mode}
            onChange={(event) => void run(() => setKeyboardMode(event.target.value as KeyboardMode))}
            disabled={disabled}
            aria-label="键盘灯效"
          >
            {modes.map((mode) => (
              <MenuItem key={mode} value={mode}>
                {keyboardModeLabel(mode)}
              </MenuItem>
            ))}
          </Select>
        </FormControl>
      </SettingsRow>

      <SettingsRow title="亮度" description="百分比，0 = 关闭，100 = 最亮">
        <Slider
          value={state.brightness}
          min={0}
          max={KEYBOARD_BRIGHTNESS_MAX}
          step={1}
          disabled={disabled}
          onChangeCommitted={(_, value) =>
            void run(() => setKeyboardBrightness(value as number).then(() => undefined))
          }
          aria-label="键盘亮度"
          sx={{ width: 150 }}
        />
      </SettingsRow>

      <SettingsRow
        title={singleZone ? "键盘颜色" : "分区颜色"}
        description={singleZone ? "整块键盘共用一个 RGB 通道" : "选择颜色后应用到全部、左侧、中部或右侧"}
      >
        <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
          <ColorPicker value={selectedColor} onChange={setColor} label="选择键盘颜色" />
          {!singleZone ? (
            <ToggleButtonGroup
              exclusive
              size="small"
              value="all"
              onChange={(_, value: KeyboardZone | null) => value && applyZoneColor(value)}
              disabled={disabled}
              aria-label="键盘分区"
            >
              {KEYBOARD_ZONES.map((value) => (
                <ToggleButton key={value} value={value}>
                  {keyboardZoneLabel(value)}
                </ToggleButton>
              ))}
            </ToggleButtonGroup>
          ) : (
            <Button
              size="small"
              variant="outlined"
              disabled={disabled}
              onClick={() => applyZoneColor("all")}
            >
              应用
            </Button>
          )}
        </Box>
      </SettingsRow>

      {!singleZone ? (
        <Box>
          <Typography sx={{ fontSize: "0.8125rem", fontWeight: 600, mb: 1 }}>单键颜色</Typography>
          <Box sx={{ overflowX: "auto", pb: 1 }}>
            <Box sx={{ display: "grid", gridTemplateColumns: "repeat(20, 22px)", gap: "4px", minWidth: 516 }}>
              {keys.flatMap((row, rowIndex) =>
                row.map((key, colIndex) => (
                  <Box
                    key={`${rowIndex}-${colIndex}`}
                    title={`第 ${rowIndex + 1} 行，第 ${colIndex + 1} 列`}
                    sx={{
                      width: 22,
                      height: 22,
                      borderRadius: 0.5,
                      border: "1px solid", borderColor: "divider",
                      backgroundColor: keysColorToHex(key),
                      opacity: disabled ? 0.55 : 1,
                      transition: "background-color 300ms ease",
                    }}
                  />
                )),
              )}
            </Box>
          </Box>
          <Typography sx={{ fontSize: "0.75rem", color: "text.disabled", mt: 0.5 }}>
            使用「分区颜色」把颜色应用到全部、左侧、中部或右侧。
          </Typography>
        </Box>
      ) : (
        <Typography sx={{ fontSize: "0.78rem", color: "text.secondary" }}>
          当前键盘为单区 RGB15，所有按键共用同一个颜色通道。侧栏的 RGB 页面提供同样的控制。
        </Typography>
      )}
    </Stack>
  );
}
