import { rgbString, type RGB } from "../lib/color";

export function modeButtonSx(color: RGB, active: boolean) {
  return {
    width: "100%",
    py: 1.25,
    fontSize: "0.75rem",
    fontWeight: active ? 700 : 600,
    letterSpacing: "0.02em",
    color: active ? "#fff" : "text.secondary",
    borderWidth: active ? 2 : 1,
    borderColor: active ? rgbString(color, 0.95) : "divider",
    backgroundColor: active ? rgbString(color, 0.42) : "action.hover",
    boxShadow: active
      ? `0 4px 18px ${rgbString(color, 0.45)}, inset 0 0 0 1px ${rgbString(color, 0.35)}`
      : "none",
    "& .MuiButton-startIcon": { mr: 0.5, ml: 0 },
    "&:hover": {
      borderWidth: active ? 2 : 1,
      backgroundColor: active ? rgbString(color, 0.52) : "divider",
      borderColor: active ? rgbString(color, 1) : "divider",
      color: "text.primary",
    },
  };
}
