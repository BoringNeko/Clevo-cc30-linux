import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";
import ActivityIcon from "@mui/icons-material/ShowChart";
import { CardHeader, GlassCard } from "./GlassCard";
import { LineChart } from "./LineChart";
import { rgbString, type ExtractedPalette } from "../lib/color";

interface TelemetryCardProps {
  palette: ExtractedPalette;
  cpuHistory: number[];
  gpuHistory: number[];
}

/**
 * Rolling fan-speed history.
 *
 * Each poll contributes one sample; the chart is smoothed by the same cubic
 * path used elsewhere. It is explicitly labelled as recent samples, not a
 * device-reported series.
 */
export function TelemetryCard({ palette, cpuHistory, gpuHistory }: TelemetryCardProps) {
  const cpuColor = rgbString(palette.primary);
  const gpuColor = rgbString(palette.secondary);

  const metrics = [
    {
      label: "CPU 风扇",
      value: cpuHistory.length ? cpuHistory[cpuHistory.length - 1] : 0,
      unit: "rpm",
      data: cpuHistory,
      color: cpuColor,
    },
    {
      label: "GPU1 风扇",
      value: gpuHistory.length ? gpuHistory[gpuHistory.length - 1] : 0,
      unit: "rpm",
      data: gpuHistory,
      color: gpuColor,
    },
  ];

  return (
    <GlassCard sx={{ gap: 2 }}>
      <CardHeader icon={<ActivityIcon sx={{ fontSize: 16 }} />} title="转速历史" />

      <Box sx={{ flex: 1, display: "grid", gridTemplateColumns: { xs: "1fr", sm: "1fr 1fr" }, gap: 2 }}>
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
                {m.value.toLocaleString()}
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
                  采集中…
                </Box>
              )}
            </Box>
          </Box>
        ))}
      </Box>
    </GlassCard>
  );
}
