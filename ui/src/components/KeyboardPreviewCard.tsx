import VisibilityIcon from "@mui/icons-material/Visibility";
import { CardHeader, GlassCard } from "./GlassCard";
import { KeyboardColorPreview } from "./KeyboardColorPreview";
import type { KeyboardMode } from "../api/daemon";
import type { RGB } from "../lib/color";

interface KeyboardPreviewCardProps {
  mode: KeyboardMode;
  color: RGB;
  brightness: number;
}

/**
 * Standalone live preview of what the keyboard is showing.
 *
 * It lives in its own card so the effect, brightness and colour controls stay
 * in the editing card while the rendered result sits beside them. The values
 * come from the editing card's in-progress state, so the preview reflects a
 * slider drag or a colour choice before it is committed to the daemon.
 */
export function KeyboardPreviewCard({ mode, color, brightness }: KeyboardPreviewCardProps) {
  return (
    <GlassCard sx={{ gap: 2 }}>
      <CardHeader icon={<VisibilityIcon sx={{ fontSize: 16 }} />} title="灯光预览" />
      <KeyboardColorPreview
        mode={mode}
        color={color}
        brightness={brightness}
      />
    </GlassCard>
  );
}
