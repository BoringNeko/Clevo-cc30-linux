import { useEffect, useRef, useState } from "react";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Popover from "@mui/material/Popover";
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";
import CheckIcon from "@mui/icons-material/Check";
import {
  hsvToRgb,
  rgbToHsv,
  rgbString,
  hexToRgbTuple,
  rgbTupleToHex,
  type HSV,
  type RGB,
} from "../lib/color";

interface ColorPickerProps {
  /** Current colour as `#rrggbb`. */
  value: string;
  onChange: (hex: string) => void;
  /** Wallpaper-derived swatches offered as one-click presets. */
  swatches?: RGB[];
  /** Accessible name and trigger tooltip. */
  label: string;
  /** Rendered inside the trigger; receives the current colour. */
  children?: (color: string) => React.ReactNode;
  /** Extra reset action shown in the footer (e.g. "default"). */
  onReset?: () => void;
  resetLabel?: string;
}

const SV_W = 200;
const SV_H = 130;
const HUE_H = 12;

/** Clamp a 0..1 ratio. */
function ratio(v: number) {
  return Math.max(0, Math.min(1, v));
}

/**
 * Glass colour picker popover, styled to the design language.
 *
 * A saturation/value square, a hue bar, a hex field and the wallpaper swatches.
 * The native `<input type="color">` chooser is not used: WebKitGTK refuses to
 * open it programmatically and its OS styling breaks the glass design. All
 * radii stay at 6px/8px and motion is limited to colour/opacity, per the spec.
 */
export function ColorPicker({
  value,
  onChange,
  swatches = [],
  label,
  children,
  onReset,
  resetLabel = "默认",
}: ColorPickerProps) {
  const anchorRef = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const [hsv, setHsv] = useState<HSV>(() => rgbToHsv(hexToRgbTuple(value) ?? [0, 0, 0]));
  const [hexDraft, setHexDraft] = useState(value);

  // Re-sync when the value changes from outside (e.g. a swatch or reset).
  useEffect(() => {
    const rgb = hexToRgbTuple(value);
    if (!rgb) return;
    setHsv(rgbToHsv(rgb));
    setHexDraft(value);
  }, [value]);

  const rgb = hsvToRgb(hsv);

  /** Track a pointer across an element and report the 0..1 x/y ratios. */
  const trackPointer = (
    el: HTMLElement,
    e: React.PointerEvent,
    onMove: (xr: number, yr: number) => void,
  ) => {
    e.preventDefault();
    const rect = el.getBoundingClientRect();
    const move = (clientX: number, clientY: number) => {
      onMove(ratio((clientX - rect.left) / rect.width), ratio((clientY - rect.top) / rect.height));
    };
    move(e.clientX, e.clientY);
    const handleMove = (ev: PointerEvent) => move(ev.clientX, ev.clientY);
    const handleUp = () => {
      window.removeEventListener("pointermove", handleMove);
      window.removeEventListener("pointerup", handleUp);
    };
    window.addEventListener("pointermove", handleMove);
    window.addEventListener("pointerup", handleUp);
  };

  const squareRef = useRef<HTMLDivElement>(null);
  const hueRef = useRef<HTMLDivElement>(null);

  const thumbSx = {
    position: "absolute" as const,
    width: 14,
    height: 14,
    borderRadius: 0.75,
    border: "2px solid rgba(255,255,255,0.9)",
    boxShadow: "0 2px 6px rgba(0,0,0,0.4)",
    transform: "translate(-50%, -50%)",
    pointerEvents: "none" as const,
    transition: "left 80ms ease, top 80ms ease",
  };

  return (
    <>
      <Box
        component="button"
        ref={anchorRef}
        type="button"
        onClick={() => setOpen(true)}
        aria-label={label}
        aria-haspopup="dialog"
        aria-expanded={open}
        sx={{
          display: "flex",
          alignItems: "center",
          gap: 1,
          p: 0.25,
          pr: 1,
          borderRadius: 1,
          border: "1px solid", borderColor: "divider",
          backgroundColor: "action.hover",
          cursor: "pointer",
          color: "text.secondary",
          font: "inherit",
          "&:hover": { backgroundColor: "action.selected", color: "text.primary" },
          transition: "background-color 300ms ease, border-color 300ms ease, color 300ms ease",
        }}
      >
        <Box
          sx={{
            width: 24,
            height: 18,
            borderRadius: 0.75,
            border: "1px solid",
            borderColor: "divider",
            backgroundColor: value,
          }}
        />
        {children ? children(value) : null}
      </Box>

      <Popover
        open={open}
        anchorEl={anchorRef.current}
        onClose={() => setOpen(false)}
        anchorOrigin={{ vertical: "bottom", horizontal: "right" }}
        transformOrigin={{ vertical: "top", horizontal: "right" }}
        slotProps={{ paper: { sx: { p: 2.5, width: 232 } } }}
      >
        <Box sx={{ display: "flex", flexDirection: "column", gap: 2 }}>
          {/* Saturation / value square */}
          <Box
            ref={squareRef}
            data-testid="sv-square"
            onPointerDown={(e) => {
              if (!squareRef.current) return;
              trackPointer(squareRef.current, e, (xr, yr) =>
                setHsv((prev) => {
                  const next = { ...prev, s: xr, v: 1 - yr };
                  const hex = rgbTupleToHex(hsvToRgb(next));
                  setHexDraft(hex);
                  onChange(hex);
                  return next;
                }),
              );
            }}
            sx={{
              position: "relative",
              width: SV_W,
              height: SV_H,
              borderRadius: 1,
              border: "1px solid", borderColor: "divider",
              backgroundColor: rgbString(hsvToRgb({ h: hsv.h, s: 1, v: 1 })),
              backgroundImage:
                "linear-gradient(to top, #000, rgba(0,0,0,0)), linear-gradient(to right, #fff, rgba(255,255,255,0))",
              cursor: "crosshair",
              touchAction: "none",
            }}
          >
            <Box
              sx={{
                ...thumbSx,
                left: `${hsv.s * 100}%`,
                top: `${(1 - hsv.v) * 100}%`,
                backgroundColor: rgbString(rgb),
              }}
            />
          </Box>

          {/* Hue bar */}
          <Box
            ref={hueRef}
            onPointerDown={(e) => {
              if (!hueRef.current) return;
              trackPointer(hueRef.current, e, (xr) =>
                setHsv((prev) => {
                  const next = { ...prev, h: xr * 360 };
                  const hex = rgbTupleToHex(hsvToRgb(next));
                  setHexDraft(hex);
                  onChange(hex);
                  return next;
                }),
              );
            }}
            sx={{
              position: "relative",
              height: HUE_H,
              borderRadius: 1,
              border: "1px solid", borderColor: "divider",
              background:
                "linear-gradient(to right, #f00, #ff0, #0f0, #0ff, #00f, #f0f, #f00)",
              cursor: "pointer",
              touchAction: "none",
            }}
          >
            <Box
              sx={{
                ...thumbSx,
                left: `${(hsv.h / 360) * 100}%`,
                top: "50%",
                backgroundColor: rgbString(hsvToRgb({ h: hsv.h, s: 1, v: 1 })),
              }}
            />
          </Box>

          {/* Hex field + current value */}
          <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
            <TextField
              value={hexDraft}
              onChange={(e) => {
                const raw = e.target.value;
                setHexDraft(raw);
                const parsed = hexToRgbTuple(raw);
                if (parsed) {
                  setHsv(rgbToHsv(parsed));
                  onChange(rgbTupleToHex(parsed));
                }
              }}
              size="small"
              slotProps={{ htmlInput: { spellCheck: false, "aria-label": "十六进制颜色" } }}
              sx={{
                flex: 1,
                "& input": {
                  fontSize: "0.75rem",
                  fontVariantNumeric: "tabular-nums",
                  py: 0.75,
                },
              }}
            />
            <Typography
              sx={{
                fontSize: "0.625rem",
                letterSpacing: "0.12em",
                textTransform: "uppercase",
                color: "text.disabled",
                whiteSpace: "nowrap",
                fontVariantNumeric: "tabular-nums",
              }}
            >
              rgb {rgb[0]}·{rgb[1]}·{rgb[2]}
            </Typography>
          </Box>

          {/* Wallpaper swatches */}
          {swatches.length > 0 ? (
            <Box>
              <Typography
                sx={{
                  fontSize: "0.625rem",
                  textTransform: "uppercase",
                  letterSpacing: "0.12em",
                  color: "text.disabled",
                  mb: 0.75,
                }}
              >
                取色板
              </Typography>
              <Box sx={{ display: "flex", gap: 0.75 }}>
                {swatches.slice(0, 8).map((c) => {
                  const hex = rgbTupleToHex(c);
                  const selected = hex === value.toLowerCase();
                  return (
                    <Box
                      key={hex}
                      component="button"
                      type="button"
                      aria-label={`使用颜色 ${hex}`}
                      onClick={() => {
                        setHsv(rgbToHsv(c));
                        setHexDraft(hex);
                        onChange(hex);
                      }}
                      sx={{
                        position: "relative",
                        flex: 1,
                        height: 20,
                        p: 0,
                        borderRadius: 1,
                        border: "1px solid",
                        borderColor: selected ? "text.primary" : "divider",
                        backgroundColor: hex,
                        cursor: "pointer",
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "center",
                        color: "#fff",
                        transition: "border-color 300ms ease",
                      }}
                    >
                      {selected ? <CheckIcon sx={{ fontSize: 12 }} /> : null}
                    </Box>
                  );
                })}
              </Box>
            </Box>
          ) : null}

          {onReset ? (
            <Button
              onClick={() => {
                onReset();
                setOpen(false);
              }}
              size="small"
              sx={{ alignSelf: "flex-start", minWidth: 0, fontSize: "0.6875rem", color: "text.secondary" }}
            >
              {resetLabel}
            </Button>
          ) : null}
        </Box>
      </Popover>
    </>
  );
}
