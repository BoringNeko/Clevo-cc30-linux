import { useEffect, useState } from "react";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import ButtonGroup from "@mui/material/ButtonGroup";
import FormControl from "@mui/material/FormControl";
import InputLabel from "@mui/material/InputLabel";
import MenuItem from "@mui/material/MenuItem";
import Select from "@mui/material/Select";
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";
import { SettingsRow } from "./SettingsRow";
import { rgbString, type ExtractedPalette } from "../lib/color";
import {
  ASPECT_RATIOS,
  DEFAULT_APPEARANCE,
  RESOLUTION_PRESETS,
  SCALE_MAX,
  SCALE_MIN,
  defaultSize,
  type Appearance,
  type AspectRatio,
} from "../theme";

interface DisplaySectionProps {
  palette: ExtractedPalette;
  appearance: Appearance;
  onAppearanceChange: (patch: Partial<Appearance>) => void;
  /** Resize the app window to the given inner size (no-op outside Tauri). */
  onResize: (width: number, height: number) => void;
}

const presetKey = (width: number, height: number) => `${width}x${height}`;

/**
 * Display settings: aspect ratio, resolution and a UI zoom.
 *
 * The ratio selects which resolution list is offered; choosing a ratio snaps the
 * size to that ratio's default. The scale is a percentage the user types
 * (50..200). All values are persisted in `Appearance` and applied live (the
 * window resize is a no-op outside Tauri).
 */
export function DisplaySection({
  palette,
  appearance,
  onAppearanceChange,
  onResize,
}: DisplaySectionProps) {
  const accent = rgbString(palette.primary);
  const presets = RESOLUTION_PRESETS[appearance.aspect];
  const current = presetKey(appearance.displayWidth, appearance.displayHeight);

  const applyResolution = (key: string) => {
    const preset = presets.find((p) => presetKey(p.width, p.height) === key);
    if (!preset) return;
    onAppearanceChange({ displayWidth: preset.width, displayHeight: preset.height });
    onResize(preset.width, preset.height);
  };

  const applyAspect = (aspect: AspectRatio) => {
    if (aspect === appearance.aspect) return;
    // Snap to that ratio's default resolution.
    const { width, height } = defaultSize(aspect);
    onAppearanceChange({ aspect, displayWidth: width, displayHeight: height });
    onResize(width, height);
  };

  return (
    <Box>
      <SettingsRow
        title="比例"
        description="应用窗口的宽高比。切换后分辨率列表会相应更新。"
      >
        <ButtonGroup size="small">
          {ASPECT_RATIOS.map((ratio) => {
            const isActive = appearance.aspect === ratio;
            return (
              <Button
                key={ratio}
                onClick={() => applyAspect(ratio)}
                variant={isActive ? "contained" : "outlined"}
                sx={{ fontSize: "0.75rem" }}
              >
                {ratio}
              </Button>
            );
          })}
        </ButtonGroup>
      </SettingsRow>

      <SettingsRow
        title="分辨率"
        description={`窗口尺寸，仅显示 ${appearance.aspect} 的预设。`}
      >
        <FormControl size="small" sx={{ minWidth: 180 }}>
          <InputLabel id="resolution-label">分辨率</InputLabel>
          <Select
            labelId="resolution-label"
            label="分辨率"
            value={current}
            onChange={(e) => applyResolution(e.target.value)}
            sx={{ fontSize: "0.8125rem", "& .MuiSelect-select": { fontVariantNumeric: "tabular-nums" } }}
          >
            {presets.map((p) => (
              <MenuItem key={presetKey(p.width, p.height)} value={presetKey(p.width, p.height)}>
                {p.width} × {p.height}
              </MenuItem>
            ))}
          </Select>
        </FormControl>
      </SettingsRow>

      <ScaleRow value={appearance.scale} onChange={(scale) => onAppearanceChange({ scale })} />

      <Button
        onClick={() => {
          const { aspect, displayWidth, displayHeight } = DEFAULT_APPEARANCE;
          onAppearanceChange({ aspect, displayWidth, displayHeight, scale: DEFAULT_APPEARANCE.scale });
          onResize(displayWidth, displayHeight);
        }}
        sx={{ mt: 2, fontSize: "0.75rem", color: accent, "&:hover": { color: "text.primary" } }}
      >
        恢复显示默认设置
      </Button>
    </Box>
  );
}

/**
 * A numeric scale input (percent). While typing, the raw text is kept so partial
 * values are editable; the committed value is clamped to `SCALE_MIN..SCALE_MAX`.
 */
function ScaleRow({ value, onChange }: { value: number; onChange: (value: number) => void }) {
  const [draft, setDraft] = useState(String(value));
  useEffect(() => setDraft(String(value)), [value]);

  const commit = (raw: string) => {
    const n = Number(raw);
    if (!Number.isFinite(n)) {
      setDraft(String(value));
      return;
    }
    const clamped = Math.min(SCALE_MAX, Math.max(SCALE_MIN, Math.round(n)));
    onChange(clamped);
    setDraft(String(clamped));
  };

  return (
    <SettingsRow title="缩放" description={`整个界面的缩放比例（${SCALE_MIN}–${SCALE_MAX}%）。`}>
      <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
        <TextField
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={(e) => commit(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") commit((e.target as HTMLInputElement).value);
          }}
          size="small"
          slotProps={{
            htmlInput: {
              inputMode: "numeric",
              "aria-label": "缩放百分比",
              style: { textAlign: "right", fontVariantNumeric: "tabular-nums" },
            },
          }}
          sx={{ width: 72, "& input": { fontSize: "0.8125rem", py: 0.75 } }}
        />
        <Typography sx={{ fontSize: "0.75rem", color: "text.secondary" }}>%</Typography>
        <Button
          onClick={() => onChange(DEFAULT_APPEARANCE.scale)}
          size="small"
          sx={{ minWidth: 0, fontSize: "0.6875rem", color: "text.secondary" }}
        >
          重置
        </Button>
      </Box>
    </SettingsRow>
  );
}
