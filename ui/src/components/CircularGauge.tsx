import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";

interface CircularGaugeProps {
  value: number;
  label: string;
  sublabel?: string;
  color: string;
  trackColor?: string;
}

export function CircularGauge({
  value,
  label,
  sublabel,
  color,
  trackColor = "currentColor",
}: CircularGaugeProps) {
  const size = 132;
  const stroke = 5;
  const radius = (size - stroke) / 2;
  const circumference = 2 * Math.PI * radius;
  const offset = circumference * (1 - Math.min(value, 100) / 100);

  return (
    <Box
      sx={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 1 }}
    >
      <Box sx={{ position: "relative", width: size, height: size }}>
        <Box
          component="svg"
          width={size}
          height={size}
          sx={{ transform: "rotate(-90deg)", color: "divider" }}
        >
          <circle
            cx={size / 2}
            cy={size / 2}
            r={radius}
            fill="none"
            stroke={trackColor}
            strokeWidth={stroke}
          />
          <circle
            cx={size / 2}
            cy={size / 2}
            r={radius}
            fill="none"
            stroke={color}
            strokeWidth={stroke}
            strokeLinecap="round"
            strokeDasharray={circumference}
            strokeDashoffset={offset}
            style={{
              transition: "stroke-dashoffset 700ms ease, stroke 500ms ease",
            }}
          />
        </Box>
        <Box
          sx={{
            position: "absolute",
            inset: 0,
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
          }}
        >
          <Typography
            sx={{
              fontSize: "1.5rem",
              fontWeight: 600,
              fontVariantNumeric: "tabular-nums",
              color: "text.primary",
            }}
          >
            {value}
            <Box
              component="span"
              sx={{ fontSize: "0.875rem", color: "text.secondary" }}
            >
              %
            </Box>
          </Typography>
          {sublabel ? (
            <Typography
              sx={{
                fontSize: "0.6875rem",
                textTransform: "uppercase",
                letterSpacing: "0.12em",
                color: "text.disabled",
              }}
            >
              {sublabel}
            </Typography>
          ) : null}
        </Box>
      </Box>
      <Typography
        sx={{
          fontSize: "0.75rem",
          fontWeight: 500,
          textTransform: "uppercase",
          letterSpacing: "0.12em",
          color: "text.secondary",
        }}
      >
        {label}
      </Typography>
    </Box>
  );
}
