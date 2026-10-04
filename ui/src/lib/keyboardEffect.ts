// Pure helpers for the static keyboard colour preview.

import type { KeyboardMode } from "../api/daemon";
import type { RGB } from "./color";

/** One preview frame: the zone colour and its relative glow intensity (0..1). */
export interface EffectFrame {
  rgb: RGB;
  intensity: number;
}

/** Mode metadata used by the lighting-mode buttons. */
export interface EffectInfo {
  mode: KeyboardMode;
  dot: string;
}

const DOT: Record<KeyboardMode, string> = {
  off: "#64748b",
  static: "#22d3ee",
};

const BLACK: RGB = [0, 0, 0];

/** The button metadata for a mode. */
export function effectInfo(mode: KeyboardMode): EffectInfo {
  return {
    mode,
    dot: DOT[mode],
  };
}

/** Compute the preview frame for the only supported lighting modes. */
export function effectFrame(mode: KeyboardMode, base: RGB): EffectFrame {
  return mode === "off" ? { rgb: BLACK, intensity: 0 } : { rgb: base, intensity: 1 };
}
