import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";
import MemoryIcon from "@mui/icons-material/Memory";
import { CardHeader, GlassCard } from "./GlassCard";
import { CircularGauge } from "./CircularGauge";
import type { FanSnapshot, HardwareUsage } from "../api/daemon";
import { rgbString, type ExtractedPalette } from "../lib/color";

interface FansCardProps {
  palette: ExtractedPalette;
  snapshot: FanSnapshot;
  usage: HardwareUsage | null;
}

/** CPU/GPU utilisation and temperature overview. */
export function FansCard({ palette, snapshot, usage }: FansCardProps) {
  const cpuColor = rgbString(palette.primary);
  const gpuColor = rgbString(palette.secondary);
  const cpu = usage?.cpu_percent ?? 0;
  const gpu = usage?.gpu_percent;
  const temp = (value: number | null | undefined) => value == null ? "n/a" : `${value}°C`;
  const freq = (value: number | null | undefined) => value == null ? "n/a" : `${value.toLocaleString()} MHz`;
  const fan = (value: number) => `${value.toLocaleString()} RPM`;

  const details = (
    label: string,
    tempValue: number | null | undefined,
    freqValue: number | null | undefined,
    fanValue: number,
  ) => (
    <Box sx={{ display: "flex", minWidth: 0, flexDirection: "column", gap: 0.75 }}>
      <Typography sx={{ fontSize: "0.875rem", lineHeight: 1.2, fontWeight: 600, color: "text.secondary", fontVariantNumeric: "tabular-nums", whiteSpace: "nowrap" }}>
        {label}
      </Typography>
      <Typography sx={{ fontSize: "0.75rem", color: "text.secondary", fontVariantNumeric: "tabular-nums", whiteSpace: "nowrap" }}>
        温度 {temp(tempValue)}
      </Typography>
      <Typography sx={{ fontSize: "0.75rem", color: "text.secondary", fontVariantNumeric: "tabular-nums", whiteSpace: "nowrap" }}>
        频率 {freq(freqValue)}
      </Typography>
      <Typography sx={{ fontSize: "0.75rem", color: "text.secondary", fontVariantNumeric: "tabular-nums", whiteSpace: "nowrap" }}>
        风扇 {fan(fanValue)}
      </Typography>
    </Box>
  );

  return (
    <GlassCard sx={{ gap: 2.5, minHeight: 220 }}>
      <CardHeader icon={<MemoryIcon sx={{ fontSize: 16 }} />} title="硬件占用"/>

      <Box sx={{ flex: 1, width: "100%", minWidth: 0, position: "relative" }}>
        <Box
          sx={{
            position: "absolute",
            left: "51%",
            top: "50%",
            transform: "translate(-50%, -50%)",
            display: "flex",
            alignItems: "center",
            gap: 4,
          }}
        >
          <Box
            sx={{
              width: 260,
              display: "grid",
              gridTemplateColumns: "132px minmax(0, 1fr)",
              alignItems: "center",
              columnGap: 1.5,
            }}
          >
            <CircularGauge value={cpu} label="" color={cpuColor} />
            {details("CPU", usage?.cpu_temp_c ?? snapshot.cpu.temp_c, usage?.cpu_freq_mhz, snapshot.cpu.rpm)}
          </Box>
          <Box
            sx={{
              width: 260,
              display: "grid",
              gridTemplateColumns: "132px minmax(0, 1fr)",
              alignItems: "center",
              columnGap: 1.5,
            }}
          >
            {gpu == null ? (
              <Box sx={{ width: 132, height: 132, display: "flex", alignItems: "center", justifyContent: "center", textAlign: "center" }}>
                <Typography sx={{ fontSize: "0.75rem", color: "text.disabled" }}>GPU 占用不可用</Typography>
              </Box>
            ) : (
              <CircularGauge value={gpu} label="" color={gpuColor} />
            )}
            {details("GPU", usage?.gpu_temp_c ?? snapshot.gpu1.temp_c, usage?.gpu_freq_mhz, snapshot.gpu1.rpm)}
          </Box>
        </Box>
      </Box>
    </GlassCard>
  );
}
