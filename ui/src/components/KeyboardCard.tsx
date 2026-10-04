import { useMemo } from "react";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Typography from "@mui/material/Typography";
import CheckIcon from "@mui/icons-material/Check";
import WbIncandescentIcon from "@mui/icons-material/WbIncandescent";
import { CardHeader, GlassCard } from "./GlassCard";
import { type KeyboardMode, type KeyboardState } from "../api/daemon";
import {
  keyboardModeLabel,
  keyboardModeSubtitle,
  keyboardSupportedModes,
  keyboardWritable,
} from "../lib/keyboard";
import { effectInfo } from "../lib/keyboardEffect";
import { rgbString, type ExtractedPalette } from "../lib/color";

interface KeyboardCardProps {
  palette: ExtractedPalette;
  state: KeyboardState;
  busy: boolean;
  onMode: (mode: KeyboardMode) => void;
}

/**
 * Keyboard lighting-mode picker.
 *
 * Capability gates the list: only the modes the daemon reports for this
 * controller are offered, and a controller that cannot be written keeps them
 * disabled. Brightness and color live in `KeyboardAppearanceCard`; the
 * live result is shown by `KeyboardPreviewCard`.
 */
export function KeyboardCard({ palette, state, busy, onMode }: KeyboardCardProps) {
  const writable = keyboardWritable(state);
  const disabled = !writable || busy;
  const modes = useMemo(() => keyboardSupportedModes(state), [state]);

  // Switching mode writes the firmware; the preview follows `state.mode` once
  // the daemon confirms it.
  const activeMode = state.mode;

  return (
    <GlassCard sx={{ gap: 2.5 }}>
      <CardHeader icon={<WbIncandescentIcon sx={{ fontSize: 16 }} />} title="键盘灯效" />

      <Box sx={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: 1 }}>
        {modes.map((mode) => {
          const info = effectInfo(mode);
          const active = activeMode === mode;
          return (
            <Button
              key={mode}
              onClick={() => onMode(mode)}
              disabled={disabled}
              variant="outlined"
              aria-pressed={active}
              aria-label={keyboardModeLabel(mode)}
              startIcon={
                active ? (
                  <CheckIcon sx={{ fontSize: 15 }} />
                ) : (
                  <Box
                    component="span"
                    sx={{ width: 7, height: 7, borderRadius: "50%", backgroundColor: info.dot }}
                  />
                )
              }
              sx={{
                display: "flex",
                flexDirection: "column",
                alignItems: "flex-start",
                gap: 0.25,
                width: "100%",
                py: 1,
                px: 1.25,
                textAlign: "left",
                fontSize: "0.75rem",
                fontWeight: active ? 700 : 600,
                letterSpacing: "0.02em",
                color: active ? "#fff" : "text.secondary",
                borderWidth: active ? 2 : 1,
                borderColor: active ? rgbString(palette.primary, 0.95) : "divider",
                backgroundColor: active ? rgbString(palette.primary, 0.42) : "action.hover",
                boxShadow: active
                  ? `0 4px 18px ${rgbString(palette.primary, 0.45)}, inset 0 0 0 1px ${rgbString(palette.primary, 0.35)}`
                  : "none",
                "& .MuiButton-startIcon": { mr: 0.5, ml: 0, position: "absolute", top: 8, right: 8 },
                "&:hover": {
                  borderWidth: active ? 2 : 1,
                  backgroundColor: active ? rgbString(palette.primary, 0.52) : "divider",
                  borderColor: active ? rgbString(palette.primary, 1) : "divider",
                  color: "text.primary",
                },
              }}
            >
              <span>{keyboardModeLabel(mode)}</span>
              <Typography component="span" sx={{ fontSize: "0.5625rem", color: "text.disabled", fontWeight: 400 }}>
                {keyboardModeSubtitle(mode)}
              </Typography>
            </Button>
          );
        })}
      </Box>
    </GlassCard>
  );
}
