import { useMemo, useState } from "react";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Slider from "@mui/material/Slider";
import ToggleButton from "@mui/material/ToggleButton";
import ToggleButtonGroup from "@mui/material/ToggleButtonGroup";
import Typography from "@mui/material/Typography";
import TuneIcon from "@mui/icons-material/Tune";
import { CardHeader, GlassCard } from "./GlassCard";
import { ColorPicker } from "./ColorPicker";
import {
  KEYBOARD_BRIGHTNESS_MAX,
  KEYBOARD_ZONES,
  type KeyboardState,
  type KeyboardZone,
} from "../api/daemon";
import { isSingleZone, keyboardWritable, keyboardZoneLabel } from "../lib/keyboard";
import { hexToRgbTuple, type ExtractedPalette } from "../lib/color";

interface KeyboardAppearanceCardProps {
  palette: ExtractedPalette;
  state: KeyboardState;
  busy: boolean;
  /** Editing state is owned by the page so the preview card can share it. */
  color: string;
  onColorChange: (hex: string) => void;
  brightness: number;
  onBrightnessChange: (level: number) => void;
  onBrightnessCommit: (level: number) => void;
  onApply: (zone: KeyboardZone, color: [number, number, number]) => void;
}

const BRIGHTNESS_MARKS = [
  { value: 0, label: "0" },
  { value: 25, label: "25" },
  { value: 50, label: "50" },
  { value: 75, label: "75" },
  { value: 100, label: "100" },
];

/** The static mode uses the selected colour. */
const COLOR_MODES: string[] = ["static"];

/**
 * Brightness and the custom colour.
 *
 * Split out of the mode card so the live preview can sit directly above the
 * controls it reflects. A single-zone controller applies the colour to its one
 * channel; a multi-zone controller also offers the zone selector.
 */
export function KeyboardAppearanceCard({
  palette,
  state,
  busy,
  color,
  onColorChange,
  brightness,
  onBrightnessChange,
  onBrightnessCommit,
  onApply,
}: KeyboardAppearanceCardProps) {
  const writable = keyboardWritable(state);
  const disabled = !writable || busy;
  const singleZone = isSingleZone(state);

  const [zone, setZone] = useState<KeyboardZone>("all");
  const activeMode = state.mode;
  const rgb = useMemo(() => hexToRgbTuple(color) ?? [0, 0, 0], [color]);

  return (
    <GlassCard sx={{ gap: 2.5 }}>
      <CardHeader icon={<TuneIcon sx={{ fontSize: 16 }} />} title="亮度与颜色" />

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
          onChange={(_, value) => onBrightnessChange(value as number)}
          onChangeCommitted={(_, value) => onBrightnessCommit(value as number)}
          aria-label="键盘亮度"
          sx={{ mt: 0.5 }}
        />
      </Box>

      {COLOR_MODES.includes(activeMode) ? (
        <Box sx={{ display: "flex", flexWrap: "wrap", alignItems: "center", justifyContent: "flex-end", gap: 1.5 }}>
          <ColorPicker value={color} onChange={onColorChange} swatches={palette.swatches} label="选择键盘颜色">
            {(value) => <Typography sx={{ fontSize: "0.75rem", fontWeight: 500 }}>{value}</Typography>}
          </ColorPicker>
          {!singleZone ? (
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
          ) : null}
          <Button
            size="small"
            variant="outlined"
            disabled={disabled}
            onClick={() => onApply(zone, rgb as [number, number, number])}
            sx={{ px: 2 }}
          >
            应用颜色
          </Button>
        </Box>
      ) : null}
    </GlassCard>
  );
}
