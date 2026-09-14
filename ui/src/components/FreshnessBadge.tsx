import Chip from "@mui/material/Chip";
import type { Freshness } from "../api/daemon";
import { statusColors, freshnessTone } from "../theme";

const LABELS: Record<Freshness, string> = {
  fresh: "实时",
  stale: "已过期",
  unknown: "未知",
};

const TOOLTIPS: Record<Freshness, string> = {
  fresh: "最近一次轮询读取成功",
  stale: "最近一次读取失败，显示的是更早的值",
  unknown: "从未成功读取",
};

/**
 * Freshness badge.
 *
 * A stale or unknown reading is never presented as live: the badge is what
 * tells the user whether the number beside it can be trusted.
 */
export function FreshnessBadge({ freshness }: { freshness: Freshness }) {
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
