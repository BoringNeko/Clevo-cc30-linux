import Box from "@mui/material/Box";
import LinearProgress from "@mui/material/LinearProgress";
import Typography from "@mui/material/Typography";
import MemoryIcon from "@mui/icons-material/Memory";
import StorageIcon from "@mui/icons-material/Storage";
import { CardHeader, GlassCard } from "./GlassCard";
import type { ExtractedPalette } from "../lib/color";

interface ResourceUsageCardProps {
  palette: ExtractedPalette;
  memoryPercent: number | null;
  swapPercent: number | null;
}

interface DiskUsageCardProps {
  palette: ExtractedPalette;
  disks: Array<{ mount_point: string; percent: number }> | null;
}

function UsageLine({ label, value, color }: { label: string; value: number | null; color: string }) {
  return (
    <Box sx={{ display: "grid", gap: 0.75 }}>
      <Box sx={{ display: "flex", justifyContent: "space-between", alignItems: "baseline" }}>
        <Typography sx={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", fontSize: "0.75rem", color: "text.secondary" }}>
          {label}
        </Typography>
        <Typography sx={{ fontSize: "0.75rem", fontWeight: 600, color: "text.primary", fontVariantNumeric: "tabular-nums" }}>
          {value == null ? "读取中" : `${value}%`}
        </Typography>
      </Box>
      <LinearProgress
        variant="determinate"
        value={value == null ? 0 : Math.min(100, Math.max(0, value))}
        sx={{
          height: 7,
          borderRadius: 4,
          backgroundColor: "action.hover",
          "& .MuiLinearProgress-bar": { borderRadius: 4, backgroundColor: color },
        }}
      />
    </Box>
  );
}

export function ResourceUsageCard({ palette, memoryPercent, swapPercent }: ResourceUsageCardProps) {
  return (
    <GlassCard sx={{ gap: 2, minHeight: 190 }}>
      <CardHeader icon={<MemoryIcon sx={{ fontSize: 16 }} />} title="内存占用" />
      <Box sx={{ display: "grid", gap: 1.75 }}>
        <UsageLine label="内存" value={memoryPercent} color={`rgb(${palette.primary.join(",")})`} />
        <UsageLine label="zram / swap" value={swapPercent} color={`rgb(${palette.secondary.join(",")})`} />
      </Box>
    </GlassCard>
  );
}

export function DiskUsageCard({ palette, disks }: DiskUsageCardProps) {
  const color = `rgb(${palette.secondary.join(",")})`;
  const useTwoColumns = (disks?.length ?? 0) > 6;
  return (
    <GlassCard sx={{ gap: 2, height: "100%", minHeight: 0 }}>
      <CardHeader icon={<StorageIcon sx={{ fontSize: 16 }} />} title="硬盘占用" />
      <Box
        sx={{
          flex: 1,
          minHeight: 0,
          display: "grid",
          gridTemplateColumns: useTwoColumns ? "repeat(2, minmax(0, 1fr))" : "1fr",
          alignItems: "start",
          alignContent: "space-between",
          columnGap: 2,
          rowGap: 1.25,
        }}
      >
        {disks === null ? (
          <Typography sx={{ fontSize: "0.1rem", color: "text.disabled" }}>读取中…</Typography>
        ) : disks.length ? disks.map((disk) => (
          <UsageLine key={disk.mount_point} label={disk.mount_point} value={disk.percent} color={color} />
        )) : (
          <Typography sx={{ fontSize: "0.75rem", color: "text.disabled" }}>后端未返回挂载信息</Typography>
        )}
      </Box>
    </GlassCard>
  );
}
