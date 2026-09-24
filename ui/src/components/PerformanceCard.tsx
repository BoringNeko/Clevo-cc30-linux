import { useState } from "react";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import SpeedIcon from "@mui/icons-material/Speed";
import CheckIcon from "@mui/icons-material/Check";
import { CardHeader, GlassCard } from "./GlassCard";
import { PERF_MODE_CHOICES, setPerfMode, type FanSnapshot } from "../api/daemon";
import { rgbString, type ExtractedPalette } from "../lib/color";

interface PerformanceCardProps {
  palette: ExtractedPalette;
  snapshot: FanSnapshot;
  onRefresh: () => void;
  onError: (message: string | null) => void;
}

export function PerformanceCard({ palette, snapshot, onRefresh, onError }: PerformanceCardProps) {
  const [busy, setBusy] = useState(false);
  const disabled = !snapshot.writable || busy;

  const apply = async (mode: string) => {
    setBusy(true);
    onError(null);
    try {
      await setPerfMode(mode);
    } catch (e) {
      onError(String(e));
    } finally {
      onRefresh();
      setBusy(false);
    }
  };

  return (
    <GlassCard sx={{ gap: 2.5 }}>
      <CardHeader icon={<SpeedIcon sx={{ fontSize: 16 }} />} title="性能模式" />
      <Box sx={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 1 }}>
        {PERF_MODE_CHOICES.map((choice) => {
          const active = snapshot.perf_mode === choice.value;
          return (
            <Button
              key={choice.value}
              onClick={() => void apply(choice.label)}
              disabled={disabled}
              variant="outlined"
              aria-pressed={active}
              startIcon={active ? <CheckIcon sx={{ fontSize: 15 }} /> : undefined}
              sx={{
                width: "100%",
                py: 1.25,
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
                "& .MuiButton-startIcon": { mr: 0.5, ml: 0 },
                "&:hover": {
                  borderWidth: active ? 2 : 1,
                  backgroundColor: active ? rgbString(palette.primary, 0.52) : "divider",
                  borderColor: active ? rgbString(palette.primary, 1) : "divider",
                  color: "text.primary",
                },
              }}
            >
              {choice.label}
            </Button>
          );
        })}
      </Box>
    </GlassCard>
  );
}
