import { useEffect, useMemo, useRef, useState } from "react";
import Alert from "@mui/material/Alert";
import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";
import KeyboardIcon from "@mui/icons-material/Keyboard";
import { KeyboardCard } from "./KeyboardCard";
import { KeyboardPreviewCard } from "./KeyboardPreviewCard";
import { KeyboardAppearanceCard } from "./KeyboardAppearanceCard";
import { KeyboardKeyEditor } from "./KeyboardKeyEditor";
import { useKeyboard } from "../hooks/useKeyboard";
import { setKeyboardBrightness, setKeyboardMode, setKeyboardZone } from "../api/daemon";
import { isSingleZone, keyboardUnavailableMessage, normaliseKeys } from "../lib/keyboard";
import { hexToRgbTuple, rgbTupleToHex, type ExtractedPalette } from "../lib/color";
import type { KeyboardState } from "../api/daemon";

interface RgbPageProps {
  palette: ExtractedPalette;
}

const POLL_MS = 4000;

/** The colour currently shown by the keyboard, or a sensible default. */
function currentColor(state: KeyboardState): string {
  const keys = normaliseKeys(state.keys);
  // Prefer a non-black key so the picker opens on something the user can see.
  for (const row of keys) {
    for (const color of row) {
      if (color[0] || color[1] || color[2]) return rgbTupleToHex(color as [number, number, number]);
    }
  }
  return "#00aaff";
}

/**
 * Keyboard RGB page.
 *
 * The page mirrors the daemon's capability model: when a writable controller is
 * present it shows the mode, brightness and colour controls plus a live
 * preview, and a per-key editor only for backends that address keys
 * individually. When the firmware advertises lights but no Linux channel
 * exists, the page says exactly that rather than offering controls that can
 * only fail.
 *
 * The editing state lives here rather than inside the controls so the preview
 * card and the controls render from the same values.
 */
export function RgbPage({ palette }: RgbPageProps) {
  const { state, error, busy, run } = useKeyboard(true, POLL_MS);

  const writable = Boolean(state?.available && state.writable);
  const singleZone = state ? isSingleZone(state) : true;

  const [color, setColor] = useState("#00aaff");
  const [brightness, setBrightness] = useState(75);
  const seeded = useRef(false);

  // Seed the colour from the keyboard once, then follow daemon brightness
  // updates without fighting a local drag.
  useEffect(() => {
    if (!state) return;
    if (!seeded.current) {
      seeded.current = true;
      setColor(currentColor(state));
    }
  }, [state]);

  useEffect(() => {
    if (state) setBrightness(state.brightness);
  }, [state?.brightness]);

  const rgb = useMemo(() => hexToRgbTuple(color) ?? ([0, 0, 0] as [number, number, number]), [color]);

  return (
    <Box
      sx={{
        display: "grid",
        gridTemplateColumns: "repeat(2, minmax(0, 1fr))",
        gap: 2,
        flex: 1,
        minHeight: 0,
        alignItems: "start",
      }}
    >
      <Box sx={{ display: "grid", gap: 2, minWidth: 0 }}>
        {error ? (
          <Alert severity="error" role="alert" data-testid="rgb-error">
            {error}
          </Alert>
        ) : null}
        {state && writable ? (
          <KeyboardCard
            palette={palette}
            state={state}
            busy={busy}
            onMode={(mode) => void run(() => setKeyboardMode(mode))}
          />
        ) : (
          <Box
            sx={{
              display: "flex",
              flexDirection: "column",
              alignItems: "center",
              justifyContent: "center",
              gap: 1,
              minHeight: 220,
              p: 2.5,
              borderRadius: 1,
              border: "1px dashed", borderColor: "divider",
            }}
          >
            <KeyboardIcon sx={{ color: "text.disabled", fontSize: 32 }} />
            <Typography sx={{ fontSize: "0.875rem", fontWeight: 600, textAlign: "center" }}>
              {keyboardUnavailableMessage(state)}
            </Typography>
            <Typography sx={{ fontSize: "0.75rem", color: "text.disabled", textAlign: "center" }}>
              接入可写的键盘灯控制器后，这里会显示亮度、灯效和颜色控制。
            </Typography>
          </Box>
        )}
      </Box>

      <Box sx={{ display: "grid", gap: 2, minWidth: 0 }}>
        {state && writable ? (
          <KeyboardPreviewCard
            mode={state.mode}
            color={rgb}
            brightness={brightness}
          />
        ) : null}
        {state && writable && state.mode !== "off" ? (
          <KeyboardAppearanceCard
            palette={palette}
            state={state}
            busy={busy}
            color={color}
            onColorChange={setColor}
            brightness={brightness}
            onBrightnessChange={setBrightness}
            onBrightnessCommit={(level) => void run(() => setKeyboardBrightness(level).then(() => undefined))}
            onApply={(zone, next) => void run(() => setKeyboardZone(zone, next))}
          />
        ) : null}
        {state && writable && !singleZone ? (
          <KeyboardKeyEditor
            palette={palette}
            state={state}
            busy={busy}
            onZone={(zone, next) => void run(() => setKeyboardZone(zone, next))}
          />
        ) : null}
      </Box>
    </Box>
  );
}
