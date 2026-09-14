import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Typography from "@mui/material/Typography";
import BoltIcon from "@mui/icons-material/Bolt";
import DashboardIcon from "@mui/icons-material/Dashboard";
import AirIcon from "@mui/icons-material/Air";
import SpeedIcon from "@mui/icons-material/Speed";
import ShowChartIcon from "@mui/icons-material/ShowChart";
import SettingsIcon from "@mui/icons-material/Settings";
import { rgbString, type ExtractedPalette } from "../lib/color";
import { glassSx, type Appearance } from "../theme";

/** Sections the sidebar can scroll to; ids match the card anchors. */
export const NAV = [
  { id: "overview", label: "概览", Icon: DashboardIcon },
  { id: "fans", label: "风扇", Icon: AirIcon },
  { id: "performance", label: "性能", Icon: SpeedIcon },
  { id: "curve", label: "风扇曲线", Icon: ShowChartIcon },
] as const;

interface SidebarProps {
  palette: ExtractedPalette;
  active: string;
  blur: boolean;
  appearance: Appearance;
  logo: string | null;
  onNavigate: (id: string) => void;
  onOpenSettings: () => void;
}

/**
 * Fixed glass sidebar: brand, section navigation and a settings entry at the
 * bottom. Wallpaper and compatibility options live inside Settings.
 */
export function Sidebar({ palette, active, blur, appearance, logo, onNavigate, onOpenSettings }: SidebarProps) {
  return (
    <Box
      component="aside"
      sx={{
        ...glassSx(blur, appearance),
        width: 240,
        flexShrink: 0,
        height: "100%",
        display: "flex",
        flexDirection: "column",
        gap: 3,
        p: 2,
      }}
    >
      <Box sx={{ display: "flex", alignItems: "center", gap: 1.25, px: 0.5 }}>
        {logo ? (
          <Box
            component="img"
            src={logo}
            alt="logo"
            sx={{ width: 36, height: 36, borderRadius: 1, objectFit: "cover", border: "1px solid", borderColor: "divider" }}
          />
        ) : (
          <Box
            sx={{
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              width: 36,
              height: 36,
              borderRadius: 1,
              border: "1px solid",
              borderColor: "divider",
              backgroundColor: rgbString(palette.primary, 0.3),
              boxShadow: `0 4px 16px ${rgbString(palette.primary, 0.35)}`,
              transition: "background-color 500ms ease, box-shadow 500ms ease",
            }}
          >
            <BoltIcon sx={{ fontSize: 18, color: "text.primary" }} />
          </Box>
        )}
        {(appearance.brandTitle || appearance.brandSubtitle) && (
          <Box sx={{ lineHeight: 1.1 }}>
            <Typography
              sx={{ fontSize: "0.875rem", fontWeight: 700, letterSpacing: "0.18em", color: "text.primary" }}
            >
              {appearance.brandTitle}
            </Typography>
            <Typography
              sx={{ fontSize: "0.625rem", fontWeight: 500, letterSpacing: "0.32em", color: "text.disabled" }}
            >
              {appearance.brandSubtitle}
            </Typography>
          </Box>
        )}
      </Box>

      <Box component="nav" sx={{ display: "flex", flexDirection: "column", gap: 0.5 }}>
        {NAV.map(({ id, label, Icon }) => {
          const isActive = id === active;
          return (
            <Button
              key={id}
              onClick={() => onNavigate(id)}
              startIcon={<Icon sx={{ fontSize: 18 }} />}
              aria-current={isActive ? "true" : undefined}
              sx={{
                justifyContent: "flex-start",
                gap: 1.5,
                px: 1.5,
                py: 1.25,
                fontSize: "0.875rem",
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
                  color: "text.primary",
                },
              }}
            >
              {label}
            </Button>
          );
        })}
      </Box>

      <Box sx={{ mt: "auto", display: "flex", flexDirection: "column", gap: 0.5 }}>
        <Button
          onClick={onOpenSettings}
          startIcon={<SettingsIcon sx={{ fontSize: 18 }} />}
          sx={{
            justifyContent: "flex-start",
            gap: 1.5,
            px: 1.5,
            py: 1.25,
            fontSize: "0.875rem",
            fontWeight: 500,
            color: "text.secondary",
            border: "1px solid transparent",
            "& .MuiButton-startIcon": { mr: 0, ml: 0 },
            "&:hover": {
              backgroundColor: "divider",
              color: "text.primary",
            },
          }}
        >
          设置
        </Button>
      </Box>
    </Box>
  );
}
