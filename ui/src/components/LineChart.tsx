import Box from "@mui/material/Box";

interface LineChartProps {
  data: number[];
  color: string;
  height?: number;
  showArea?: boolean;
}

function buildPath(data: number[], width: number, height: number, pad = 6) {
  if (data.length < 2) {
    return { line: "", area: "" };
  }
  const max = Math.max(...data);
  const min = Math.min(...data);
  const range = max - min || 1;
  const stepX = (width - pad * 2) / (data.length - 1);

  const points = data.map((d, i) => {
    const x = pad + i * stepX;
    const y = pad + (1 - (d - min) / range) * (height - pad * 2);
    return { x, y };
  });

  let line = `M ${points[0].x} ${points[0].y}`;
  for (let i = 0; i < points.length - 1; i++) {
    const p0 = points[i];
    const p1 = points[i + 1];
    const cx = (p0.x + p1.x) / 2;
    line += ` C ${cx} ${p0.y}, ${cx} ${p1.y}, ${p1.x} ${p1.y}`;
  }

  const area = `${line} L ${points[points.length - 1].x} ${height} L ${points[0].x} ${height} Z`;
  return { line, area };
}

/**
 * Small area/line chart used for the RPM and curve telemetry.
 *
 * A smooth cubic path with a soft gradient fill; the stroke is a restrained
 * solid accent (no glow), per the design language.
 */
export function LineChart({ data, color, height = 74, showArea }: LineChartProps) {
  const width = 320;
  const { line, area } = buildPath(data, width, height);
  const gradientId = `grad-${color.replace(/[^a-z0-9]/gi, "")}`;

  return (
    <Box
      component="svg"
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="none"
      sx={{ width: "100%", height: "100%" }}
    >
      <defs>
        <linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor={color} stopOpacity={0.28} />
          <stop offset="100%" stopColor={color} stopOpacity={0} />
        </linearGradient>
      </defs>
      {showArea ? <path d={area} fill={`url(#${gradientId})`} /> : null}
      <path
        d={line}
        fill="none"
        stroke={color}
        strokeWidth={1.75}
        strokeLinecap="round"
        strokeLinejoin="round"
        style={{ transition: "stroke 500ms ease" }}
      />
    </Box>
  );
}
