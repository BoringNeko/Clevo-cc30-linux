import { useState } from "react";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import SpeedIcon from "@mui/icons-material/Speed";
import CheckIcon from "@mui/icons-material/Check";
import { CardHeader, GlassCard } from "./GlassCard";
import { PERF_MODE_CHOICES, setPerfMode, type FanSnapshot } from "../api/daemon";
import type { ExtractedPalette } from "../lib/color";
import { modeButtonSx } from "./modeButtonSx";

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
              sx={modeButtonSx(palette.primary, active)}
            >
              {choice.label}
            </Button>
          );
        })}
      </Box>
    </GlassCard>
  );
}
