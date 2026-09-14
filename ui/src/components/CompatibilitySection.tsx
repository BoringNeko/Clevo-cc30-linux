import Alert from "@mui/material/Alert";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import FormControl from "@mui/material/FormControl";
import FormControlLabel from "@mui/material/FormControlLabel";
import InputLabel from "@mui/material/InputLabel";
import MenuItem from "@mui/material/MenuItem";
import Select from "@mui/material/Select";
import Slider from "@mui/material/Slider";
import Switch from "@mui/material/Switch";
import Typography from "@mui/material/Typography";
import {
  writeCompatibilityPrefs,
  writeBlurSetting,
  type BlurSetting,
  type CompatibilityPrefs,
} from "../hooks/useAppSettings";
import { rgbString, type ExtractedPalette } from "../lib/color";
import type { Appearance } from "../theme";

interface CompatibilitySectionProps {
  palette: ExtractedPalette;
  blurSetting: BlurSetting;
  onBlurSettingChange: (value: BlurSetting) => void;
  appearance: Appearance;
  onAppearanceChange: (patch: Partial<Appearance>) => void;
  compatibility: CompatibilityPrefs;
  onCompatibilityChange: (value: CompatibilityPrefs) => void;
}

function Row({
  title,
  description,
  children,
}: {
  title: string;
  description?: string;
  children: React.ReactNode;
}) {
  return (
    <Box
      sx={{
        display: "flex",
        alignItems: "flex-start",
        justifyContent: "space-between",
        gap: 3,
        py: 1.75,
        borderBottom: "1px solid", borderBottomColor: "divider",
      }}
    >
      <Box sx={{ minWidth: 0 }}>
        <Typography sx={{ fontSize: "0.8125rem", fontWeight: 600, color: "text.primary" }}>
          {title}
        </Typography>
        {description ? (
          <Typography sx={{ fontSize: "0.75rem", color: "text.disabled", mt: 0.25 }}>
            {description}
          </Typography>
        ) : null}
      </Box>
      <Box sx={{ flexShrink: 0 }}>{children}</Box>
    </Box>
  );
}

/**
 * Compatibility options for the WebView.
 *
 * Backend and software rendering are process-wide, launch-time environment
 * variables (`GDK_BACKEND`, `WEBKIT_DISABLE_DMABUF_RENDERER`,
 * `WEBKIT_DISABLE_COMPOSITING_MODE`); they cannot be changed in a running
 * WebView. The UI therefore persists the choice and states plainly that it
 * applies on the next launch. Blur, by contrast, is a CSS choice and applies
 * immediately.
 */
export function CompatibilitySection({
  palette,
  blurSetting,
  onBlurSettingChange,
  appearance,
  onAppearanceChange,
  compatibility,
  onCompatibilityChange,
}: CompatibilitySectionProps) {
  const accent = rgbString(palette.primary);

  const update = (patch: Partial<CompatibilityPrefs>) => {
    const next = { ...compatibility, ...patch };
    onCompatibilityChange(next);
    writeCompatibilityPrefs(next);
  };

  const restartHint = (
    <Alert
      severity="info"
      sx={{
        mt: 2,
        fontSize: "0.75rem",
        backgroundColor: "action.hover",
        color: "text.secondary",
        border: "1px solid", borderColor: "divider",
        "& .MuiAlert-icon": { color: accent },
      }}
    >
      显示后端与软件渲染的更改需要重启应用后生效。关闭并重新打开应用即可应用
      （也可自行设置对应的环境变量覆盖）。
    </Alert>
  );

  return (
    <Box>
      <Row
        title="显示后端"
        description="窗口使用的显示服务器。Wayland 是原生方式，但部分合成器（Hyprland、Sway）会导致 WebKitGTK 崩溃；X11 经由 XWayland 回退，兼容性更好，但模糊效果可能较差。"
      >
        <FormControl size="small" sx={{ minWidth: 150 }}>
          <InputLabel id="backend-label">后端</InputLabel>
          <Select
            labelId="backend-label"
            label="后端"
            value={compatibility.backend}
            onChange={(e) => update({ backend: e.target.value as CompatibilityPrefs["backend"] })}
          >
            <MenuItem value="auto">Auto（推荐）</MenuItem>
            <MenuItem value="wayland">Wayland (GDK_BACKEND=wayland)</MenuItem>
            <MenuItem value="x11">X11 (GDK_BACKEND=x11)</MenuItem>
          </Select>
        </FormControl>
      </Row>

      <Row
        title="软件渲染"
        description="禁用 DMA-BUF GPU 渲染路径（WEBKIT_DISABLE_DMABUF_RENDERER=1）。可修复 wlroots 系合成器上的崩溃，代价是失去毛玻璃模糊且 CPU 占用更高。"
      >
        <FormControlLabel
          control={
            <Switch
              checked={compatibility.softwareRendering}
              onChange={(e) => update({ softwareRendering: e.target.checked })}
            />
          }
          label={compatibility.softwareRendering ? "开" : "关"}
          sx={{ "& .MuiFormControlLabel-label": { fontSize: "0.75rem", color: "text.secondary" } }}
        />
      </Row>

      <Row
        title="玻璃模糊"
        description="毛玻璃背景效果。Auto 在支持时启用。若卡片显得过于透明（软件渲染下常见），可关闭；界面会变得更不透明。"
      >
        <FormControl size="small" sx={{ minWidth: 120 }}>
          <InputLabel id="blur-label">模糊</InputLabel>
          <Select
            labelId="blur-label"
            label="模糊"
            value={blurSetting}
            onChange={(e) => {
              const value = e.target.value as BlurSetting;
              writeBlurSetting(value);
              onBlurSettingChange(value);
            }}
          >
            <MenuItem value="auto">Auto</MenuItem>
            <MenuItem value="on">开</MenuItem>
            <MenuItem value="off">关</MenuItem>
          </Select>
        </FormControl>
      </Row>

      <Row
        title="模糊度"
        description="毛玻璃的模糊半径（px）。模糊关闭时不可调。"
      >
        <Box sx={{ display: "flex", alignItems: "center", gap: 1.5, width: 200 }}>
          <Slider
            min={0}
            max={40}
            step={1}
            value={appearance.blurPx}
            disabled={blurSetting === "off"}
            onChange={(_, v) => onAppearanceChange({ blurPx: v as number })}
            sx={{ color: accent, "& .MuiSlider-thumb": { width: 14, height: 14, borderRadius: 0.75 } }}
          />
          <Typography sx={{ fontSize: "0.75rem", width: 40, color: "text.secondary", fontVariantNumeric: "tabular-nums" }}>
            {appearance.blurPx}px
          </Typography>
        </Box>
      </Row>

      {restartHint}

      <Box sx={{ mt: 2, fontSize: "0.6875rem", color: "text.disabled", lineHeight: 1.7 }}>
        <div>Auto → 不设置任何变量（由 WebKit 决定）。</div>
        <div>Wayland → <code>GDK_BACKEND=wayland</code></div>
        <div>X11 → <code>GDK_BACKEND=x11</code></div>
        <div>软件渲染 → <code>WEBKIT_DISABLE_DMABUF_RENDERER=1</code></div>
      </Box>

      <Button
        onClick={() => {
          update({ backend: "auto", softwareRendering: false });
          writeBlurSetting("auto");
          onBlurSettingChange("auto");
        }}
        sx={{ mt: 2, fontSize: "0.75rem", color: "text.secondary", "&:hover": { color: "text.primary" } }}
      >
        恢复兼容性默认设置
      </Button>
    </Box>
  );
}
