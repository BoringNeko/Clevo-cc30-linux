import Chip from "@mui/material/Chip";
import type { Freshness } from "../api/daemon";
import { statusColors, freshnessTone } from "../theme";

type Unfresh = Exclude<Freshness, "fresh">;

const LABELS: Record<Unfresh, string> = {
  stale: "已过期",
  unknown: "未知",
};

const TOOLTIPS: Record<Unfresh, string> = {
  stale: "读数已过期或最近一次读取失败",
  unknown: "从未成功读取",
};

/**
 * Warn when a reading is stale or unavailable.
 */
export function FreshnessBadge({ freshness }: { freshness: Freshness }) {
  if (freshness === "fresh") return null;
  const tone = statusColors[freshnessTone(freshness)];
  return (
    <Chip
      data-testid="freshness"
      size="small"
      label={LABELS[freshness]}
      title={TOOLTIPS[freshness]}
      sx={{
        height: 22,
        fontSize: "0.625rem",
        fontWeight: 500,
        borderRadius: 1,
        border: `1px solid ${tone.borderColor}`,
        color: tone.color,
        backgroundColor: tone.backgroundColor,
        "& .MuiChip-label": { px: 1, py: 0.5 },
      }}
    />
  );
}
