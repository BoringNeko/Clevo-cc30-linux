import { useState } from "react";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Typography from "@mui/material/Typography";
import SpeedIcon from "@mui/icons-material/Speed";
import CheckIcon from "@mui/icons-material/Check";
import { CardHeader, GlassCard } from "./GlassCard";
import {
  FAN_MODE_CHOICES,
  PERF_MODE_CHOICES,
  setFanMode,
  setPerfMode,
  type FanSnapshot,
} from "../api/daemon";
import { rgbString, type ExtractedPalette } from "../lib/color";

interface PerformanceCardProps {
  palette: ExtractedPalette;
  snapshot: FanSnapshot;
  onRefresh: () => void;
  onError: (message: string | null) => void;
}

/**
 * Fan and performance mode selection.
 *
 * Buttons reflect the daemon's reported mode (`255` = not reported, shown as
 * "unknown"). A click sends a write through the daemon; the result is always
 * followed by a refresh so a denied write does not appear to have taken effect.
 */
export function PerformanceCard({
  palette,
  snapshot,
  onRefresh,
  onError,
}: PerformanceCardProps) {
  const [busy, setBusy] = useState(false);
  const disabled = !snapshot.writable || busy;

  const apply = async (fn: () => Promise<number>) => {
    setBusy(true);
    onError(null);
    try {
      await fn();
    } catch (e) {
      onError(String(e));
    } finally {
      onRefresh();
      setBusy(false);
    }
  };

  const modeButton = (
    value: number,
    label: string,
    current: number,
    onClick: () => void,
  ) => {
    const isActive = current === value;
    return (
      <Button
        key={value}
        onClick={onClick}
        disabled={disabled}
        variant="outlined"
        aria-pressed={isActive}
        startIcon={isActive ? <CheckIcon sx={{ fontSize: 15 }} /> : undefined}
        sx={{
          py: 1.25,
          fontSize: "0.75rem",
          fontWeight: isActive ? 700 : 600,
          letterSpacing: "0.02em",
          color: isActive ? "#fff" : "text.secondary",
          borderWidth: isActive ? 2 : 1,
          borderColor: isActive ? rgbString(palette.primary, 0.95) : "divider",
          backgroundColor: isActive ? rgbString(palette.primary, 0.42) : "action.hover",
          boxShadow: isActive
            ? `0 4px 18px ${rgbString(palette.primary, 0.45)}, inset 0 0 0 1px ${rgbString(palette.primary, 0.35)}`
            : "none",
          "& .MuiButton-startIcon": { mr: 0.5, ml: 0 },
          "&:hover": {
            borderWidth: isActive ? 2 : 1,
            backgroundColor: isActive
              ? rgbString(palette.primary, 0.52)
              : "divider",
            borderColor: isActive
              ? rgbString(palette.primary, 1)
              : "divider",
            color: "text.primary",
          },
        }}
      >
        {label}
      </Button>
    );
  };

  return (
    <GlassCard sx={{ gap: 2.5 }}>
      <CardHeader icon={<SpeedIcon sx={{ fontSize: 16 }} />} title="模式" />

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
          风扇模式
        </Typography>
        <Box sx={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 1 }}>
          {FAN_MODE_CHOICES.map((c) =>
            modeButton(c.value, c.label, snapshot.fan_mode, () =>
              void apply(() => setFanMode(c.label)),
            ),
          )}
        </Box>
      </Box>

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
          性能模式
        </Typography>
        <Box sx={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 1 }}>
          {PERF_MODE_CHOICES.map((c) =>
            modeButton(c.value, c.label, snapshot.perf_mode, () =>
              void apply(() => setPerfMode(c.label)),
            ),
          )}
        </Box>
      </Box>

    </GlassCard>
  );
}
