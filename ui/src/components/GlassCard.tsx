import type { ReactNode } from "react";
import Box from "@mui/material/Box";
import Card from "@mui/material/Card";
import Typography from "@mui/material/Typography";

interface GlassCardProps {
  children: ReactNode;
  sx?: object;
}

export function GlassCard({ children, sx }: GlassCardProps) {
  return (
    <Card
      sx={{
        display: "flex",
        flexDirection: "column",
        p: 2.5,
        height: "auto",
        width: "100%",
        minWidth: 0,
        boxSizing: "border-box",
        ...sx,
      }}
    >
      {children}
    </Card>
  );
}

interface CardHeaderProps {
  icon: ReactNode;
  title: string;
  hint?: string;
}

export function CardHeader({ icon, title, hint }: CardHeaderProps) {
  return (
    <Box
      sx={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
      }}
    >
      <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
        <Box
          sx={{
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            width: 28,
            height: 28,
            borderRadius: 1,
            border: "1px solid", borderColor: "divider",
            backgroundColor: "divider",
            color: "text.secondary",
          }}
        >
          {icon}
        </Box>
        <Typography
          sx={{
            fontSize: "0.875rem",
            fontWeight: 600,
            letterSpacing: "0.02em",
            color: "text.primary",
          }}
        >
          {title}
        </Typography>
      </Box>
      {hint ? (
        <Typography
          sx={{
            fontSize: "0.6875rem",
            fontWeight: 500,
            textTransform: "uppercase",
            letterSpacing: "0.12em",
            color: "text.disabled",
          }}
        >
          {hint}
        </Typography>
      ) : null}
    </Box>
  );
}
