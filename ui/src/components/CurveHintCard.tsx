import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";
import ShowChartIcon from "@mui/icons-material/ShowChart";
import { CardHeader, GlassCard } from "./GlassCard";
import { rgbString, type ExtractedPalette } from "../lib/color";

interface CurveHintCardProps {
  palette: ExtractedPalette;
}

/**
 * The fan-curve card's place, shown while the fan mode is not `customize`.
 *
 * The curve editor is deliberately hidden outside that mode: a curve does
 * nothing until the firmware is told to use it, and showing an editable chart
 * that the EC ignores would be misleading. This card says what to do instead of
 * leaving a hole in the grid.
 */
export function CurveHintCard({ palette }: CurveHintCardProps) {
  const accent = rgbString(palette.primary, 0.95);

  return (
    <GlassCard sx={{ gap: 2 }}>
      <CardHeader icon={<ShowChartIcon sx={{ fontSize: 16 }} />} title="风扇曲线" hint="customize" />

      <Box
        sx={{
          flex: 1,
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "center",
          gap: 1,
          borderRadius: 1,
          border: "1px dashed", borderColor: "divider",
          backgroundColor: "action.hover",
          px: 2.5,
          textAlign: "center",
        }}
      >
        <Typography sx={{ fontSize: "0.75rem", color: "text.secondary" }}>
          将风扇模式切换到{" "}
          <Box component="span" sx={{ color: accent, fontWeight: 600 }}>
            customize
          </Box>{" "}
          以编辑自定义曲线
        </Typography>
        <Typography sx={{ fontSize: "0.6875rem", color: "text.disabled" }}>
          其他模式下固件不使用自定义曲线，因此这里不显示编辑器。
        </Typography>
      </Box>
    </GlassCard>
  );
}
