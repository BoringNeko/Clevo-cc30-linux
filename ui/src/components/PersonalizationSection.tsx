import { useRef } from "react";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import ButtonGroup from "@mui/material/ButtonGroup";
import Slider from "@mui/material/Slider";
import Switch from "@mui/material/Switch";
import FormControlLabel from "@mui/material/FormControlLabel";
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";
import ImageIcon from "@mui/icons-material/Image";
import RestartAltIcon from "@mui/icons-material/RestartAlt";
import DarkModeIcon from "@mui/icons-material/DarkMode";
import LightModeIcon from "@mui/icons-material/LightMode";
import { ColorPicker } from "./ColorPicker";
import { rgbTupleToHex, type ExtractedPalette } from "../lib/color";
import { DEFAULT_MOTION_SPEED, MOTION_SPEED_MAX, MOTION_SPEED_MIN } from "../motion";
import type { Appearance } from "../theme";

interface PersonalizationSectionProps {
  palette: ExtractedPalette;
  onWallpaperChange: (file: File) => void;
  onResetWallpaper: () => void;
  wallpaperIsCustom: boolean;
  appearance: Appearance;
  onAppearanceChange: (patch: Partial<Appearance>) => void;
  logo: string | null;
  logoIsCustom: boolean;
  onLogoChange: (file: File) => void;
  onLogoReset: () => void;
}

const rgbToHex = rgbTupleToHex;

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
        borderBottom: "1px solid rgba(127,127,127,0.18)",
      }}
    >
      <Box sx={{ minWidth: 0 }}>
        <Typography sx={{ fontSize: "0.8125rem", fontWeight: 600 }}>
          {title}
        </Typography>
        {description ? (
          <Typography sx={{ fontSize: "0.75rem", color: "text.secondary", mt: 0.25 }}>
            {description}
          </Typography>
        ) : null}
      </Box>
      <Box sx={{ flexShrink: 0 }}>{children}</Box>
    </Box>
  );
}

/**
 * A colour swatch that opens the custom picker; `value === null` means
 * "default". The reset button restores the mode/wallpaper-derived colour.
 */
function ColorSetting({
  value,
  fallback,
  swatches,
  label,
  onChange,
}: {
  value: string | null;
  fallback: string;
  swatches: ExtractedPalette["swatches"];
  label: string;
  onChange: (value: string | null) => void;
}) {
  const shown = value ?? fallback;
  return (
    <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
      <ColorPicker
        value={shown}
        swatches={swatches}
        label={label}
        onChange={(hex) => onChange(hex)}
        onReset={value === null ? undefined : () => onChange(null)}
      />
      <Button
        onClick={() => onChange(null)}
        disabled={value === null}
        size="small"
        sx={{ fontSize: "0.6875rem", color: value === null ? "text.disabled" : "text.secondary", minWidth: 0 }}
      >
        默认
      </Button>
    </Box>
  );
}

/**
 * Personalization: wallpaper, light/dark mode and colour overrides for the
 * accent, glass surface and text. Colour overrides apply immediately; "默认"
 * restores the mode/wallpaper-derived value.
 */
export function PersonalizationSection({
  palette,
  onWallpaperChange,
  onResetWallpaper,
  wallpaperIsCustom,
  appearance,
  onAppearanceChange,
  logo,
  logoIsCustom,
  onLogoChange,
  onLogoReset,
}: PersonalizationSectionProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const logoInputRef = useRef<HTMLInputElement>(null);

  return (
    <Box>
      <Row
        title="主题模式"
        description="深色或浅色玻璃界面。浅色模式使用浅色玻璃底与深色文字。"
      >
        <ButtonGroup size="small">
          <Button
            onClick={() => onAppearanceChange({ mode: "dark" })}
            startIcon={<DarkModeIcon sx={{ fontSize: 16 }} />}
            variant={appearance.mode === "dark" ? "contained" : "outlined"}
            sx={{ fontSize: "0.75rem" }}
          >
            深色
          </Button>
          <Button
            onClick={() => onAppearanceChange({ mode: "light" })}
            startIcon={<LightModeIcon sx={{ fontSize: 16 }} />}
            variant={appearance.mode === "light" ? "contained" : "outlined"}
            sx={{ fontSize: "0.75rem" }}
          >
            浅色
          </Button>
        </ButtonGroup>
      </Row>

      <Row
        title="自定义壁纸"
        description="作为仪表盘背景。系统会从壁纸提取主色作为强调色（可被下方覆盖）。"
      >
        <Box sx={{ display: "flex", gap: 1 }}>
          <input
            ref={inputRef}
            type="file"
            accept="image/*"
            hidden
            onChange={(e) => {
              const file = e.target.files?.[0];
              if (file) onWallpaperChange(file);
              e.target.value = "";
            }}
          />
          <Button
            onClick={() => inputRef.current?.click()}
            startIcon={<ImageIcon sx={{ fontSize: 16 }} />}
            variant="outlined"
            sx={{ fontSize: "0.75rem" }}
          >
            选择图片…
          </Button>
          <Button
            onClick={onResetWallpaper}
            disabled={!wallpaperIsCustom}
            startIcon={<RestartAltIcon sx={{ fontSize: 16 }} />}
            sx={{ fontSize: "0.75rem", color: "text.secondary" }}
          >
            重置
          </Button>
        </Box>
      </Row>

      <Row
        title="强调色"
        description="按钮、图表与高亮的主色。默认取自壁纸。"
      >
        <ColorSetting
          value={appearance.accent}
          fallback={rgbToHex(palette.primary)}
          swatches={palette.swatches}
          label="强调色"
          onChange={(v) => onAppearanceChange({ accent: v })}
        />
      </Row>

      <Row
        title="卡片颜色"
        description="卡片与侧栏的玻璃底色。默认随主题模式。"
      >
        <ColorSetting
          value={appearance.surface}
          fallback={appearance.mode === "dark" ? "#000000" : "#ffffff"}
          swatches={palette.swatches}
          label="卡片颜色"
          onChange={(v) => onAppearanceChange({ surface: v })}
        />
      </Row>

      <Row
        title="透明度"
        description="玻璃表面的不透明度。越低越透（配合模糊更明显）。"
      >
        <Box sx={{ display: "flex", alignItems: "center", gap: 1.5, width: 220 }}>
          <Slider
            min={0.05}
            max={1}
            step={0.01}
            value={appearance.opacity ?? (appearance.mode === "dark" ? 0.42 : 0.55)}
            onChange={(_, v) => onAppearanceChange({ opacity: v as number })}
            sx={{ color: "primary.main", "& .MuiSlider-thumb": { width: 14, height: 14, borderRadius: 0.75 } }}
          />
          <Typography sx={{ fontSize: "0.75rem", width: 44, color: "text.secondary", fontVariantNumeric: "tabular-nums" }}>
            {Math.round((appearance.opacity ?? (appearance.mode === "dark" ? 0.42 : 0.55)) * 100)}%
          </Typography>
          <Button
            onClick={() => onAppearanceChange({ opacity: null })}
            disabled={appearance.opacity === null}
            size="small"
            sx={{ minWidth: 0, fontSize: "0.6875rem", color: appearance.opacity === null ? "text.disabled" : "text.secondary" }}
          >
            默认
          </Button>
        </Box>
      </Row>

      <Row
        title="动画"
        description="关闭页面和设置切换动画，卡片内容会直接切换。"
      >
        <FormControlLabel
          control={
            <Switch
              checked={appearance.animationsEnabled}
              onChange={(e) => onAppearanceChange({ animationsEnabled: e.target.checked })}
            />
          }
          label={appearance.animationsEnabled ? "开" : "关"}
          sx={{ "& .MuiFormControlLabel-label": { fontSize: "0.75rem", color: "text.secondary" } }}
        />
      </Row>

      <Row
        title="动画速度"
        description={`页面和设置切换的动画速度（${MOTION_SPEED_MIN}%–${MOTION_SPEED_MAX}%）。数值越高切换越快。`}
      >
        <Box sx={{ display: "flex", alignItems: "center", gap: 1.5, width: 220 }}>
          <Slider
            aria-label="动画速度"
            min={MOTION_SPEED_MIN}
            max={MOTION_SPEED_MAX}
            step={10}
            value={appearance.animationSpeed}
            onChange={(_, v) => onAppearanceChange({ animationSpeed: v as number })}
            sx={{ color: "primary.main", "& .MuiSlider-thumb": { width: 14, height: 14, borderRadius: 0.75 } }}
          />
          <Typography sx={{ fontSize: "0.75rem", width: 44, color: "text.secondary", fontVariantNumeric: "tabular-nums" }}>
            {appearance.animationSpeed}%
          </Typography>
          <Button
            onClick={() => onAppearanceChange({ animationSpeed: DEFAULT_MOTION_SPEED })}
            disabled={appearance.animationSpeed === DEFAULT_MOTION_SPEED}
            size="small"
            sx={{ minWidth: 0, fontSize: "0.6875rem", color: appearance.animationSpeed === DEFAULT_MOTION_SPEED ? "text.disabled" : "text.secondary" }}
          >
            默认
          </Button>
        </Box>
      </Row>

      <Row
        title="文字颜色"
        description="界面文字颜色。默认随主题模式。"
      >
        <ColorSetting
          value={appearance.textColor}
          fallback={appearance.mode === "dark" ? "#ffffff" : "#111417"}
          swatches={palette.swatches}
          label="文字颜色"
          onChange={(v) => onAppearanceChange({ textColor: v })}
        />
      </Row>

      <Row title="品牌文字" description="左上角显示的名称，留空则隐藏。">
        <Box sx={{ display: "flex", gap: 1 }}>
          <TextField
            size="small"
            value={appearance.brandTitle}
            onChange={(e) => onAppearanceChange({ brandTitle: e.target.value })}
            placeholder="CLEVO"
            sx={{ width: 120, "& input": { fontSize: "0.75rem" } }}
          />
          <TextField
            size="small"
            value={appearance.brandSubtitle}
            onChange={(e) => onAppearanceChange({ brandSubtitle: e.target.value })}
            placeholder="CONTROL"
            sx={{ width: 120, "& input": { fontSize: "0.75rem" } }}
          />
        </Box>
      </Row>

      <Row title="图标" description="左上角图标，可换成图片。默认使用内置 logo。">
        <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
          {logo ? (
            <Box
              component="img"
              src={logo}
              alt="logo"
              sx={{ width: 40, height: 28, objectFit: "cover", borderRadius: 1, border: "1px solid", borderColor: "divider" }}
            />
          ) : null}
          <input
            ref={logoInputRef}
            type="file"
            accept="image/*"
            hidden
            onChange={(e) => {
              const file = e.target.files?.[0];
              if (file) onLogoChange(file);
              e.target.value = "";
            }}
          />
          <Button
            onClick={() => logoInputRef.current?.click()}
            startIcon={<ImageIcon sx={{ fontSize: 16 }} />}
            variant="outlined"
            sx={{ fontSize: "0.75rem" }}
          >
            选择图片…
          </Button>
          <Button
            onClick={onLogoReset}
            disabled={!logoIsCustom}
            size="small"
            sx={{ fontSize: "0.6875rem", color: logoIsCustom ? "text.secondary" : "text.disabled", minWidth: 0 }}
          >
            重置
          </Button>
        </Box>
      </Row>

    </Box>
  );
}
