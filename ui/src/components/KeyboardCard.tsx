import { useEffect, useMemo, useState } from "react";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Slider from "@mui/material/Slider";
import ToggleButton from "@mui/material/ToggleButton";
import ToggleButtonGroup from "@mui/material/ToggleButtonGroup";
import Typography from "@mui/material/Typography";
import CheckIcon from "@mui/icons-material/Check";
import WbIncandescentIcon from "@mui/icons-material/WbIncandescent";
import { CardHeader, GlassCard } from "./GlassCard";
import { ColorPicker } from "./ColorPicker";
import { KeyboardStage } from "./KeyboardStage";
import { KeyboardPreview } from "./KeyboardPreview";
import { KEYBOARD_BRIGHTNESS_MAX, KEYBOARD_ZONES, type KeyboardMode, type KeyboardState, type KeyboardZone } from "../api/daemon";
import {
  applyZone,
  isSingleZone,
  keyboardModeLabel,
  keyboardModeSubtitle,
  keyboardSupportedModes,
  keyboardWritable,
  keyboardZoneLabel,
  normaliseKeys,
  type KeyboardKeys,
} from "../lib/keyboard";
import { effectInfo } from "../lib/keyboardEffect";
import { hexToRgbTuple, rgbString, rgbTupleToHex, type ExtractedPalette } from "../lib/color";

interface KeyboardCardProps {
  palette: ExtractedPalette;
  state: KeyboardState;
  busy: boolean;
  onMode: (mode: KeyboardMode) => void;
  onBrightness: (level: number) => void;
  onColor: (zone: KeyboardZone, color: [number, number, number]) => void;
}

const BRIGHTNESS_MARKS = [
  { value: 0, label: "0" },
  { value: 25, label: "25" },
  { value: 50, label: "50" },
  { value: 75, label: "75" },
  { value: 100, label: "100" },
];

/** The colour currently shown by the keyboard, or a sensible default. */
function currentColor(state: KeyboardState): string {
  const keys = normaliseKeys(state.keys);
  // Prefer a non-black key so the picker opens on something the user can see.
  for (const row of keys) {
    for (const color of row) {
      if (color[0] || color[1] || color[2]) return rgbTupleToHex(color as [number, number, number]);
    }
  }
  return "#00aaff";
}

/**
 * Keyboard backlight studio: a live single-zone preview, the firmware effects,
 * brightness and colour.
 *
 * Capability gates every control: only the effects the daemon reports for this
 * controller are offered, a controller that cannot be written keeps its
 * controls disabled, and a single-zone controller shows one colour channel with
 * no zone selector rather than pretending left/middle/right are separate.
 */
export function KeyboardCard({ palette, state, busy, onMode, onBrightness, onColor }: KeyboardCardProps) {
  const writable = keyboardWritable(state);
  const disabled = !writable || busy;
  const singleZone = isSingleZone(state);
  const modes = useMemo(() => keyboardSupportedModes(state), [state]);

  const [color, setColor] = useState(() => currentColor(state));
  const [zone, setZone] = useState<KeyboardZone>("all");
  const [keys, setKeys] = useState<KeyboardKeys>(() => normaliseKeys(state.keys));
  // Track the slider locally so it follows the drag; the committed value is
  // what goes to the daemon, and the refresh afterwards re-syncs it.
  const [brightness, setBrightness] = useState(state.brightness);

  // Keep the preview in step with what the daemon reports after a write.
  useEffect(() => {
    setKeys(normaliseKeys(state.keys));
  }, [state.keys]);

  useEffect(() => {
    setBrightness(state.brightness);
  }, [state.brightness]);

  // Switching effect writes the firmware; the preview follows `state.mode` once
  // the daemon confirms it, so a rejected effect never animates as if applied.
  const activeMode = state.mode;
  const rgb = useMemo(() => hexToRgbTuple(color) ?? [0, 0, 0], [color]);

  const apply = () => {
    onColor(zone, rgb as [number, number, number]);
    setKeys((current) => applyZone(current, zone, rgb as [number, number, number]));
  };

  return (
    <GlassCard sx={{ gap: 2.5 }}>
      <CardHeader icon={<WbIncandescentIcon sx={{ fontSize: 16 }} />} title="键盘灯效" hint="键盘背光" />

      <KeyboardStage mode={activeMode} color={rgb as [number, number, number]} brightness={brightness} />

      <Box>
        <Typography
          sx={{
            fontSize: "0.625rem",
            textTransform: "uppercase",
            letterSpacing: "0.12em",
            color: "text.disabled",
            mb: 1,
          }}
        >
          单区灯效{modes.length > 0 ? ` · ${modes.length} 种` : ""}
        </Typography>
        <Box sx={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: 1 }}>
          {modes.map((mode) => {
            const info = effectInfo(mode);
            const active = activeMode === mode;
            return (
              <Button
                key={mode}
                onClick={() => onMode(mode)}
                disabled={disabled}
                variant="outlined"
                aria-pressed={active}
                aria-label={keyboardModeLabel(mode)}
                startIcon={
                  active ? (
                    <CheckIcon sx={{ fontSize: 15 }} />
                  ) : (
                    <Box
                      component="span"
                      sx={{ width: 7, height: 7, borderRadius: "50%", backgroundColor: info.dot }}
                    />
                  )
                }
                sx={{
                  display: "flex",
                  flexDirection: "column",
                  alignItems: "flex-start",
                  gap: 0.25,
                  width: "100%",
                  py: 1,
                  px: 1.25,
                  textAlign: "left",
                  fontSize: "0.75rem",
                  fontWeight: active ? 700 : 600,
                  letterSpacing: "0.02em",
                  color: active ? "#fff" : "text.secondary",
                  borderWidth: active ? 2 : 1,
                  borderColor: active ? rgbString(palette.primary, 0.95) : "divider",
                  backgroundColor: active ? rgbString(palette.primary, 0.42) : "action.hover",
                  boxShadow: active
                    ? `0 4px 18px ${rgbString(palette.primary, 0.45)}, inset 0 0 0 1px ${rgbString(palette.primary, 0.35)}`
                    : "none",
                  "& .MuiButton-startIcon": { mr: 0.5, ml: 0, position: "absolute", top: 8, right: 8 },
                  "&:hover": {
                    borderWidth: active ? 2 : 1,
                    backgroundColor: active ? rgbString(palette.primary, 0.52) : "divider",
                    borderColor: active ? rgbString(palette.primary, 1) : "divider",
                    color: "text.primary",
                  },
                }}
              >
                <span>{keyboardModeLabel(mode)}</span>
                <Typography component="span" sx={{ fontSize: "0.5625rem", color: "text.disabled", fontWeight: 400 }}>
                  {keyboardModeSubtitle(mode)}
                </Typography>
              </Button>
            );
          })}
        </Box>
      </Box>

      <Box>
        <Box sx={{ display: "flex", alignItems: "baseline", justifyContent: "space-between" }}>
          <Typography sx={{ fontSize: "0.75rem", fontWeight: 500, color: "text.secondary" }}>
            亮度
          </Typography>
          <Typography
            sx={{ fontSize: "0.75rem", color: "text.disabled", fontVariantNumeric: "tabular-nums" }}
          >
            {brightness}%
          </Typography>
        </Box>
        <Slider
          value={brightness}
          min={0}
          max={KEYBOARD_BRIGHTNESS_MAX}
          step={1}
          marks={BRIGHTNESS_MARKS}
          disabled={disabled}
          onChange={(_, value) => setBrightness(value as number)}
          onChangeCommitted={(_, value) => onBrightness(value as number)}
          aria-label="键盘亮度"
          sx={{ mt: 0.5 }}
        />
      </Box>

      <Box sx={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 1.5 }}>
        <ColorPicker value={color} onChange={setColor} swatches={palette.swatches} label="选择键盘颜色">
          {(value) => (
            <Typography sx={{ fontSize: "0.75rem", fontWeight: 500 }}>{value}</Typography>
          )}
        </ColorPicker>
        {singleZone ? (
          <Typography sx={{ fontSize: "0.75rem", color: "text.disabled" }}>
            整块键盘共用一个颜色通道
          </Typography>
        ) : (
          <ToggleButtonGroup
            exclusive
            size="small"
            value={zone}
            onChange={(_, value: KeyboardZone | null) => value && setZone(value)}
            disabled={disabled}
            aria-label="键盘分区"
          >
            {KEYBOARD_ZONES.map((value) => (
              <ToggleButton key={value} value={value}>
                {keyboardZoneLabel(value)}
              </ToggleButton>
            ))}
          </ToggleButtonGroup>
        )}
        <Button size="small" variant="outlined" disabled={disabled} onClick={apply} sx={{ px: 2 }}>
          应用颜色
        </Button>
      </Box>

      {/*
       * A single-zone board has one physical channel, so the animated stage
       * above already *is* the colour readout; a second grid would only repeat
       * it. The 6x20 grid is for controllers that address keys individually.
       */}
      {!singleZone ? (
        <Box>
          <Typography
            sx={{
              fontSize: "0.625rem",
              textTransform: "uppercase",
              letterSpacing: "0.12em",
              color: "text.disabled",
              mb: 1,
            }}
          >
            分区颜色
          </Typography>
          <KeyboardPreview keys={keys} />
        </Box>
      ) : null}
    </GlassCard>
  );
}
