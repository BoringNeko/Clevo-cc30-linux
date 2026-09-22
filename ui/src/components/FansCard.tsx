import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";
import AirIcon from "@mui/icons-material/Air";
import ThermostatIcon from "@mui/icons-material/Thermostat";
import { CardHeader, GlassCard } from "./GlassCard";
import { CircularGauge } from "./CircularGauge";
import type { FanSnapshot } from "../api/daemon";
import { rgbString, type ExtractedPalette } from "../lib/color";

/** The machine's maximum observed fan speed, used to scale the gauges. */
const MAX_RPM = 6200;

interface FansCardProps {
  palette: ExtractedPalette;
  snapshot: FanSnapshot;
}

/**
 * CPU/GPU fan speeds as circular gauges.
 *
 * Gauge value is rpm scaled to the machine's observed maximum; the raw rpm is
 * shown in the centre label.
 */
export function FansCard({ palette, snapshot }: FansCardProps) {
  const accent = rgbString(palette.primary);
  const accentSoft = rgbString(palette.primary, 0.75);
  const pct = (rpm: number) => Math.min(100, Math.round((rpm / MAX_RPM) * 100));

  return (
    <GlassCard sx={{ gap: 2.5 }}>
      <CardHeader icon={<AirIcon sx={{ fontSize: 16 }} />} title="风扇" />

      <Box sx={{ flex: 1, display: "flex", alignItems: "center", justifyContent: "space-around", gap: 2 }}>
        {snapshot.cpu.available ? (
          <CircularGauge
            value={pct(snapshot.cpu.rpm)}
            label={`CPU · ${snapshot.cpu.rpm} rpm`}
            sublabel="转速"
            color={accent}
          />
        ) : (
          <Typography sx={{ fontSize: "0.75rem", color: "text.disabled" }}>
            CPU 不可用
          </Typography>
        )}
        {snapshot.gpu1.available ? (
          <CircularGauge
            value={pct(snapshot.gpu1.rpm)}
            label={`GPU1 · ${snapshot.gpu1.rpm} rpm`}
            sublabel="转速"
            color={accentSoft}
          />
        ) : (
          <Typography sx={{ fontSize: "0.75rem", color: "text.disabled" }}>
            GPU1 不可用
          </Typography>
        )}
      </Box>

      <Box
        sx={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          borderTop: "1px solid", borderTopColor: "divider",
          pt: 1.5,
          fontSize: "0.6875rem",
          color: "text.disabled",
        }}
      >
        <Box sx={{ display: "flex", alignItems: "center", gap: 0.75 }}>
          <ThermostatIcon sx={{ fontSize: 13 }} /> CPU{" "}
          {snapshot.cpu.temp_c === null ? "n/a" : `${snapshot.cpu.temp_c}°`} · GPU{" "}
          {snapshot.gpu1.temp_c === null ? "n/a" : `${snapshot.gpu1.temp_c}°`}
        </Box>
      </Box>
    </GlassCard>
  );
}
