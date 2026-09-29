import Alert from "@mui/material/Alert";
import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";
import KeyboardIcon from "@mui/icons-material/Keyboard";
import { KeyboardCard } from "./KeyboardCard";
import { KeyboardCapabilityCard } from "./KeyboardCapabilityCard";
import { KeyboardKeyEditor } from "./KeyboardKeyEditor";
import { useKeyboard } from "../hooks/useKeyboard";
import { setKeyboardBrightness, setKeyboardMode, setKeyboardZone } from "../api/daemon";
import { isSingleZone, keyboardUnavailableMessage } from "../lib/keyboard";
import type { ExtractedPalette } from "../lib/color";

interface RgbPageProps {
  palette: ExtractedPalette;
}

const POLL_MS = 4000;

/**
 * Keyboard RGB page.
 *
 * The page mirrors the daemon's capability model: when a writable controller is
 * present it shows the effect, brightness and colour controls, plus a per-key
 * editor only for backends that address keys individually. When the firmware
 * advertises lights but no Linux channel exists, the page says exactly that
 * rather than offering controls that can only fail.
 */
export function RgbPage({ palette }: RgbPageProps) {
  const { state, error, busy, run } = useKeyboard(true, POLL_MS);

  const writable = Boolean(state?.available && state.writable);
  const singleZone = state ? isSingleZone(state) : true;

  return (
    <Box
      sx={{
        display: "grid",
        gridTemplateColumns: "1fr 1fr",
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
            onBrightness={(level) => void run(() => setKeyboardBrightness(level).then(() => undefined))}
            onColor={(zone, color) => void run(() => setKeyboardZone(zone, color))}
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
        <KeyboardCapabilityCard state={state} />
        {state && writable && !singleZone ? (
          <KeyboardKeyEditor
            palette={palette}
            state={state}
            busy={busy}
            onZone={(zone, color) => void run(() => setKeyboardZone(zone, color))}
          />
        ) : null}
      </Box>
    </Box>
  );
}
