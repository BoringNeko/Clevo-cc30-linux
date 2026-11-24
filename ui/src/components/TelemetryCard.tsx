import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";
import ActivityIcon from "@mui/icons-material/ShowChart";
import type { Freshness } from "../api/daemon";
import { CardHeader, GlassCard } from "./GlassCard";
import { FreshnessBadge } from "./FreshnessBadge";
import { LineChart } from "./LineChart";
import { rgbString, type ExtractedPalette } from "../lib/color";

interface TelemetryCardProps {
  palette: ExtractedPalette;
  cpuHistory: number[];
  gpuHistory: number[];
  freshness: Freshness;
}

/**
 * Rolling fan-speed history.
 *
 * Plots recent reads of the daemon cache, not a device-reported series.
 */
export function TelemetryCard({ palette, cpuHistory, gpuHistory, freshness }: TelemetryCardProps) {
  const cpuColor = rgbString(palette.primary);
  const gpuColor = rgbString(palette.secondary);

  const metrics = [
    {
      label: "CPU 风扇",
      value: cpuHistory[cpuHistory.length - 1],
      unit: "rpm",
      data: cpuHistory,
      color: cpuColor,
    },
    {
      label: "GPU1 风扇",
      value: gpuHistory[gpuHistory.length - 1],
      unit: "rpm",
      data: gpuHistory,
      color: gpuColor,
    },
  ];

  return (
    <GlassCard sx={{ gap: 2 }}>
      <Box sx={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: 1 }}>
        <CardHeader icon={<ActivityIcon sx={{ fontSize: 16 }} />} title="转速历史" />
        <FreshnessBadge freshness={freshness} />
      </Box>

      {/* Fixed two columns: the design surface is a constant 1600x900, so a
          viewport breakpoint would collapse this on a scaled-down window. */}
      <Box sx={{ flex: 1, display: "grid", gridTemplateColumns: "1fr 1fr", gap: 2 }}>
        {metrics.map((m) => (
          <Box key={m.label} sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
            <Box sx={{ display: "flex", alignItems: "baseline", justifyContent: "space-between" }}>
              <Typography sx={{ fontSize: "0.75rem", fontWeight: 500, color: "text.secondary" }}>
                {m.label}
              </Typography>
              <Typography
                sx={{
                  fontSize: "0.875rem",
                  fontWeight: 600,
                  fontVariantNumeric: "tabular-nums",
                  color: "text.primary",
                }}
              >
                {m.value?.toLocaleString() ?? "—"}
                <Box component="span" sx={{ ml: 0.25, fontSize: "0.625rem", color: "text.disabled" }}>
                  {m.unit}
                </Box>
              </Typography>
            </Box>
            <Box
              sx={{
                height: 74,
                width: "100%",
                borderRadius: 1,
                border: "1px solid", borderColor: "divider",
                backgroundColor: "action.hover",
                px: 0.5,
              }}
            >
              {m.data.length >= 2 ? (
                <LineChart data={m.data} color={m.color} showArea />
              ) : (
                <Box
                  sx={{
                    height: "100%",
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    fontSize: "0.625rem",
                    color: "text.disabled",
                  }}
                >
                  {freshness === "unknown" ? "等待转速读数" : "暂无历史"}
                </Box>
              )}
            </Box>
          </Box>
        ))}
      </Box>
    </GlassCard>
  );
}
