import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";

/** A titled settings row: label/description on the left, control on the right. */
export function SettingsRow({
  title,
  description,
  children,
}: {
  title: string;
  description?: string;
  children: React.ReactNode;
}) {
  return (
    <Box
      sx={{
        display: "flex",
        alignItems: "flex-start",
        justifyContent: "space-between",
        gap: 3,
        py: 1.75,
        borderBottom: "1px solid", borderBottomColor: "divider",
      }}
    >
      <Box sx={{ minWidth: 0 }}>
        <Typography sx={{ fontSize: "0.8125rem", fontWeight: 600, color: "text.primary" }}>
          {title}
        </Typography>
        {description ? (
          <Typography sx={{ fontSize: "0.75rem", color: "text.disabled", mt: 0.25 }}>
            {description}
          </Typography>
        ) : null}
      </Box>
      <Box sx={{ flexShrink: 0 }}>{children}</Box>
    </Box>
  );
}
