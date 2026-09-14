import { useState } from "react";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Dialog from "@mui/material/Dialog";
import Divider from "@mui/material/Divider";
import IconButton from "@mui/material/IconButton";
import Typography from "@mui/material/Typography";
import CloseIcon from "@mui/icons-material/Close";
import PaletteIcon from "@mui/icons-material/Palette";
import TuneIcon from "@mui/icons-material/Tune";
import { CompatibilitySection } from "./CompatibilitySection";
import { PersonalizationSection } from "./PersonalizationSection";
import { useTheme } from "@mui/material/styles";
import { rgbString, type ExtractedPalette } from "../lib/color";
import type { BlurSetting } from "../hooks/useAppSettings";
import type { CompatibilityPrefs } from "../hooks/useAppSettings";
import type { Appearance } from "../theme";

const SECTIONS = [
  { id: "personalization", label: "个性化", Icon: PaletteIcon },
  { id: "compatibility", label: "兼容性", Icon: TuneIcon },
] as const;

type SectionId = (typeof SECTIONS)[number]["id"];

interface SettingsDialogProps {
  open: boolean;
  onClose: () => void;
  palette: ExtractedPalette;
  onWallpaperChange: (file: File) => void;
  onResetWallpaper: () => void;
  wallpaperIsCustom: boolean;
  /** Current blur preference and setter (applied immediately). */
  blurSetting: BlurSetting;
  onBlurSettingChange: (value: BlurSetting) => void;
  appearance: Appearance;
  onAppearanceChange: (patch: Partial<Appearance>) => void;
  logo: string | null;
  logoIsCustom: boolean;
  onLogoChange: (file: File) => void;
  onLogoReset: () => void;
  /** Backend / software-rendering prefs (applied on next launch). */
  compatibility: CompatibilityPrefs;
  onCompatibilityChange: (value: CompatibilityPrefs) => void;
}

/**
 * Settings, presented as a window inside the app: a glass panel with its own
 * left navigation and a right-hand options pane.
 *
 * The blur setting applies immediately. Backend and software rendering are
 * launch-time choices and only take effect on the next start, which the UI
 * states explicitly.
 */
export function SettingsDialog({
  open,
  onClose,
  palette,
  onWallpaperChange,
  onResetWallpaper,
  wallpaperIsCustom,
  blurSetting,
  onBlurSettingChange,
  appearance,
  onAppearanceChange,
  logo,
  logoIsCustom,
  onLogoChange,
  onLogoReset,
  compatibility,
  onCompatibilityChange,
}: SettingsDialogProps) {
  const [active, setActive] = useState<SectionId>("personalization");
  const theme = useTheme();
  const dark = theme.palette.mode === "dark";
  
  return (
    <Dialog
      open={open}
      onClose={onClose}
      maxWidth="md"
      fullWidth
      slotProps={{
        backdrop: { sx: { backgroundColor: "rgba(0,0,0,0.55)", backdropFilter: "blur(4px)" } },
        paper: {
          sx: {
            backgroundColor: dark ? "rgba(14,14,18,0.92)" : "rgba(250,250,252,0.95)",
            backdropFilter: "blur(24px)",
            WebkitBackdropFilter: "blur(24px)",
            border: "1px solid", borderColor: "divider",
            borderRadius: 2,
            boxShadow: dark ? "0 24px 60px rgba(0,0,0,0.6)" : "0 24px 60px rgba(0,0,0,0.25)",
            backgroundImage: "none",
          },
        },
      }}
    >
      <Box sx={{ display: "flex", height: 520 }}>
        <Box
          component="nav"
          sx={{
            width: 200,
            flexShrink: 0,
            borderRight: "1px solid", borderRightColor: "divider",
            p: 2,
            display: "flex",
            flexDirection: "column",
            gap: 0.5,
          }}
        >
          <Typography
            sx={{
              fontSize: "0.625rem",
              textTransform: "uppercase",
              letterSpacing: "0.14em",
              color: "text.disabled",
              px: 1,
              mb: 1,
            }}
          >
            设置
          </Typography>
          {SECTIONS.map(({ id, label, Icon }) => {
            const isActive = id === active;
            return (
              <Button
                key={id}
                onClick={() => setActive(id)}
                startIcon={<Icon sx={{ fontSize: 17 }} />}
                sx={{
                  justifyContent: "flex-start",
                  gap: 1.25,
                  px: 1.25,
                  py: 1,
                  fontSize: "0.8125rem",
                  fontWeight: 500,
                  color: isActive ? "#fff" : "text.secondary",
                  border: "1px solid",
                  borderColor: isActive ? rgbString(palette.primary, 0.4) : "transparent",
                  backgroundColor: isActive ? rgbString(palette.primary, 0.28) : "transparent",
                  "& .MuiButton-startIcon": { mr: 0, ml: 0 },
                  "&:hover": {
                    backgroundColor: isActive
                      ? rgbString(palette.primary, 0.34)
                      : "divider",
                  },
                }}
              >
                {label}
              </Button>
            );
          })}
        </Box>

        <Box sx={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column" }}>
          <Box
            sx={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              px: 2.5,
              py: 1.5,
            }}
          >
            <Typography sx={{ fontSize: "1rem", fontWeight: 600, color: "text.primary" }}>
              {SECTIONS.find((s) => s.id === active)?.label}
            </Typography>
            <IconButton
              onClick={onClose}
              aria-label="关闭设置"
              sx={{ color: "text.secondary", "&:hover": { color: "text.primary" } }}
            >
              <CloseIcon fontSize="small" />
            </IconButton>
          </Box>
          <Divider sx={{ borderColor: "divider" }} />

          <Box sx={{ flex: 1, overflowY: "auto", p: 2.5 }}>
            {active === "personalization" ? (
              <PersonalizationSection
                palette={palette}
                onWallpaperChange={onWallpaperChange}
                onResetWallpaper={onResetWallpaper}
                wallpaperIsCustom={wallpaperIsCustom}
                appearance={appearance}
                onAppearanceChange={onAppearanceChange}
                logo={logo}
                logoIsCustom={logoIsCustom}
                onLogoChange={onLogoChange}
                onLogoReset={onLogoReset}
              />
            ) : (
              <CompatibilitySection
                palette={palette}
                blurSetting={blurSetting}
                onBlurSettingChange={onBlurSettingChange}
                appearance={appearance}
                onAppearanceChange={onAppearanceChange}
                compatibility={compatibility}
                onCompatibilityChange={onCompatibilityChange}
              />
            )}
          </Box>
        </Box>
      </Box>
    </Dialog>
  );
}
