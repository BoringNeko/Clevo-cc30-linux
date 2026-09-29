import Box from "@mui/material/Box";
import { keysColorToHex, type KeyboardKeys } from "../lib/keyboard";

/** Cell size and gap for the preview grid. */
const CELL = 18;
const GAP = 3;

/**
 * Read-only 6x20 keyboard preview.
 *
 * Each cell is the colour the daemon last wrote for that key. On a single-zone
 * controller every cell carries the same colour, which is exactly the point:
 * the preview shows what the hardware actually does rather than implying the
 * keys can differ.
 */
export function KeyboardPreview({ keys }: { keys: KeyboardKeys }) {
  return (
    <Box
      role="img"
      aria-label="键盘灯颜色预览"
      sx={{
        display: "grid",
        gridTemplateColumns: `repeat(20, ${CELL}px)`,
        gap: `${GAP}px`,
        justifyContent: "center",
        p: 1.25,
        borderRadius: 1,
        border: "1px solid", borderColor: "divider",
        backgroundColor: "action.hover",
      }}
    >
      {keys.flatMap((row, rowIndex) =>
        row.map((color, colIndex) => (
          <Box
            key={`${rowIndex}-${colIndex}`}
            sx={{
              width: CELL,
              height: CELL,
              borderRadius: 0.75,
              border: "1px solid",
              borderColor: "divider",
              backgroundColor: keysColorToHex(color),
              transition: "background-color 300ms ease",
            }}
          />
        )),
      )}
    </Box>
  );
}
