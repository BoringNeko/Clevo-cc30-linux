import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";
import ShowChartIcon from "@mui/icons-material/ShowChart";
import { CardHeader, GlassCard } from "./GlassCard";
import type { FanCurve } from "../api/daemon";
import { rgbString, type ExtractedPalette } from "../lib/color";

interface CurveCardProps {
  palette: ExtractedPalette;
  curve: FanCurve;
}

const W = 320;
const H = 150;
const PAD = 18;

/** Map a (temp, duty) curve to the SVG polyline the design uses. */
function toPath(points: Array<{ temp: number; duty_pct: number }>): string {
  if (points.length === 0) return "";
  return points
    .map((p, i) => {
      const x = PAD + (p.temp / 100) * (W - 2 * PAD);
      const y = H - PAD - (p.duty_pct / 100) * (H - 2 * PAD);
      return `${i === 0 ? "M" : "L"}${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
}

/**
 * The fan curve as two restrained polylines (CPU and GPU1).
 *
 * The firmware interpolates between the four points; this shows the actual
 * points so the user sees exactly what is programmed.
 */
export function CurveCard({ palette, curve }: CurveCardProps) {
  const cpuColor = rgbString(palette.primary);
  const gpuColor = rgbString(palette.secondary);
  const series = [
    { label: "CPU", points: curve.cpu, color: cpuColor },
    { label: "GPU1", points: curve.gpu1, color: gpuColor },
  ];

  return (
    <GlassCard sx={{ gap: 2 }}>
      <CardHeader icon={<ShowChartIcon sx={{ fontSize: 16 }} />} title="风扇曲线" />

      <Box
        sx={{
          flex: 1,
          borderRadius: 1,
          border: "1px solid", borderColor: "divider",
          backgroundColor: "action.hover",
          px: 0.5,
          py: 0.5,
        }}
      >
        <Box
          component="svg"
          viewBox={`0 0 ${W} ${H}`}
          sx={{ width: "100%", height: "100%", color: "text.primary" }}
        >
          <line x1={PAD} y1={H - PAD} x2={W - PAD} y2={H - PAD} stroke="currentColor" strokeOpacity={0.18} />
          <line x1={PAD} y1={PAD} x2={PAD} y2={H - PAD} stroke="currentColor" strokeOpacity={0.18} />
          {series.map((s) => (
            <g key={s.label}>
              <path
                d={toPath(s.points)}
                fill="none"
                stroke={s.color}
                strokeWidth={1.75}
                strokeLinecap="round"
                strokeLinejoin="round"
                style={{ transition: "stroke 500ms ease" }}
              />
              {s.points.map((p) => {
                const x = PAD + (p.temp / 100) * (W - 2 * PAD);
                const y = H - PAD - (p.duty_pct / 100) * (H - 2 * PAD);
                return <circle key={`${s.label}-${p.temp}`} cx={x} cy={y} r={2.5} fill={s.color} />;
              })}
            </g>
          ))}
        </Box>
      </Box>

      <Box sx={{ display: "flex", flexDirection: "column", gap: 0.5 }}>
        {series.map((s) => (
          <Typography
            key={s.label}
            sx={{ fontSize: "0.6875rem", color: "text.secondary" }}
          >
            <Box component="span" sx={{ color: s.color, fontWeight: 600 }}>
              {s.label}:
            </Box>{" "}
            {s.points.map((p) => `(${p.temp}°C,${p.duty_pct}%)`).join(" ")}
          </Typography>
        ))}
      </Box>
    </GlassCard>
  );
}
