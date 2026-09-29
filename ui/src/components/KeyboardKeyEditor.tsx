import { useMemo, useState } from "react";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Typography from "@mui/material/Typography";
import GridOnIcon from "@mui/icons-material/GridOn";
import { CardHeader, GlassCard } from "./GlassCard";
import { ColorPicker } from "./ColorPicker";
import {
  KEYBOARD_ZONES,
  type KeyboardState,
  type KeyboardZone,
} from "../api/daemon";
import {
  applyZone,
  keyboardWritable,
  keyboardZoneLabel,
  normaliseKeys,
  type KeyboardKeys,
} from "../lib/keyboard";
import { hexToRgbTuple, rgbString, type ExtractedPalette } from "../lib/color";

interface KeyboardKeyEditorProps {
  palette: ExtractedPalette;
  state: KeyboardState;
  busy: boolean;
  onZone: (zone: KeyboardZone, color: [number, number, number]) => void;
}

const CELL = 20;
const GAP = 3;

/** Fill every key, or one logical zone, with a single colour. */
function colorPreview(color: [number, number, number]): string {
  return `rgb(${color[0]}, ${color[1]}, ${color[2]})`;
}

/**
 * Per-key colour editor for controllers with independent keys.
 *
 * Only the USB HID backend addresses keys individually; a single-zone RGB15
 * machine never reaches this card, because painting 120 cells that all change
 * together would misrepresent the hardware.
 *
 * This is a bulk editor rather than a click-a-key editor: with 120 keys of 20px
 * it is the way to get a whole zone coloured, and it keeps every target at a
 * usable size inside the fixed design surface.
 */
export function KeyboardKeyEditor({ palette, state, busy, onZone }: KeyboardKeyEditorProps) {
  const writable = keyboardWritable(state);
  const [color, setColor] = useState("#00aaff");
  const [zone, setZone] = useState<KeyboardZone>("all");
  const [keys, setKeys] = useState<KeyboardKeys>(() => normaliseKeys(state.keys));

  const rgb = useMemo(() => hexToRgbTuple(color) ?? [0, 0, 0], [color]);

  const apply = () => {
    onZone(zone, rgb as [number, number, number]);
    setKeys((current) => applyZone(current, zone, rgb as [number, number, number]));
  };

  return (
    <GlassCard sx={{ gap: 2.5 }}>
      <CardHeader icon={<GridOnIcon sx={{ fontSize: 16 }} />} title="分区颜色" hint="6 × 20" />

      <Box sx={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 1.5 }}>
        <ColorPicker value={color} onChange={setColor} swatches={palette.swatches} label="选择分区颜色">
          {(value) => (
            <Typography sx={{ fontSize: "0.75rem", fontWeight: 500 }}>{value}</Typography>
          )}
        </ColorPicker>
        <Box sx={{ display: "flex", gap: 0.75 }}>
          {KEYBOARD_ZONES.map((value) => {
            const active = value === zone;
            return (
              <Button
                key={value}
                onClick={() => setZone(value)}
                disabled={!writable || busy}
                variant="outlined"
                aria-pressed={active}
                sx={{
                  minWidth: 0,
                  px: 1.5,
                  py: 0.5,
                  fontSize: "0.75rem",
                  color: active ? "#fff" : "text.secondary",
                  borderColor: active ? rgbString(palette.primary, 0.9) : "divider",
                  backgroundColor: active ? rgbString(palette.primary, 0.35) : "action.hover",
                  "&:hover": {
                    borderColor: active ? rgbString(palette.primary, 1) : "divider",
                    backgroundColor: active ? rgbString(palette.primary, 0.45) : "divider",
                    color: "text.primary",
                  },
                }}
              >
                {keyboardZoneLabel(value)}
              </Button>
            );
          })}
        </Box>
        <Button size="small" variant="outlined" disabled={!writable || busy} onClick={apply} sx={{ px: 2 }}>
          应用到{keyboardZoneLabel(zone)}
        </Button>
      </Box>

      <Box sx={{ overflowX: "auto", pb: 0.5 }}>
        <Box
          sx={{
            display: "grid",
            gridTemplateColumns: `repeat(20, ${CELL}px)`,
            gap: `${GAP}px`,
            minWidth: 20 * CELL + 19 * GAP,
            mx: "auto",
          }}
        >
          {keys.flatMap((row, rowIndex) =>
            row.map((key, colIndex) => (
              <Box
                key={`${rowIndex}-${colIndex}`}
                title={`第 ${rowIndex + 1} 行，第 ${colIndex + 1} 列`}
                sx={{
                  width: CELL,
                  height: CELL,
                  borderRadius: 0.75,
                  border: "1px solid",
                  borderColor: "divider",
                  backgroundColor: colorPreview(key as [number, number, number]),
                  transition: "background-color 300ms ease",
                  opacity: writable ? 1 : 0.55,
                }}
              />
            )),
          )}
        </Box>
      </Box>
    </GlassCard>
  );
}
