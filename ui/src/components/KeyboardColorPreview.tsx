import Box from "@mui/material/Box";
import type { CSSProperties } from "react";
import type { KeyboardMode } from "../api/daemon";
import { effectFrame } from "../lib/keyboardEffect";
import type { RGB } from "../lib/color";

interface KeyboardColorPreviewProps {
  mode: KeyboardMode;
  color: RGB;
  brightness: number;
}

/** Compact effect preview that shows only the current light colour. */
export function KeyboardColorPreview({
  mode,
  color,
  brightness,
}: KeyboardColorPreviewProps) {
  const initial = effectFrame(mode, color);

  return (
    <Box
      role="img"
      aria-label="键盘颜色预览"
      style={
        {
          "--preview-r": initial.rgb[0],
          "--preview-g": initial.rgb[1],
          "--preview-b": initial.rgb[2],
          "--preview-glow": Math.max(0, Math.min(1, (brightness / 100) * initial.intensity)),
        } as CSSProperties
      }
      sx={{
        position: "relative",
        height: 88,
        overflow: "hidden",
        borderRadius: 1,
        border: "1px solid",
        borderColor: "divider",
        backgroundColor: "action.hover",
        boxShadow: "inset 0 1px 0 rgba(255,255,255,0.06)",
      }}
    >
      <Box
        aria-hidden
        sx={{
          position: "absolute",
          inset: -24,
          backgroundColor: "rgb(var(--preview-r), var(--preview-g), var(--preview-b))",
          opacity: "calc(var(--preview-glow) * 0.42)",
          filter: "blur(28px)",
          transition: "background-color 120ms linear, opacity 120ms linear",
        }}
      />
      <Box
        aria-hidden
        sx={{
          position: "absolute",
          inset: "18px 14px",
          borderRadius: 0.75,
          backgroundColor: "rgb(var(--preview-r), var(--preview-g), var(--preview-b))",
          opacity: "calc(0.2 + var(--preview-glow) * 0.8)",
          boxShadow:
            "0 0 22px rgba(var(--preview-r), var(--preview-g), var(--preview-b), calc(var(--preview-glow) * 0.55))",
          transition: "background-color 120ms linear, opacity 120ms linear, box-shadow 120ms linear",
        }}
      />
    </Box>
  );
}
