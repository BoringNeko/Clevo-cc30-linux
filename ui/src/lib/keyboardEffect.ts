// Pure helpers for the RGB studio preview.
//
// The preview only *visualises* what the firmware is doing; the effects run on
// the EC. Everything here is deterministic-in-time so a given `mode`, colour and
// elapsed time always produces the same frame — that makes it testable and
// keeps the animation honest about what the hardware shows (a single zone, one
// synchronised colour).

import type { KeyboardMode } from "../api/daemon";
import { hsvToRgb, type RGB } from "./color";

/** One preview frame: the zone colour and its relative glow intensity (0..1). */
export interface EffectFrame {
  rgb: RGB;
  intensity: number;
}

/** Effect metadata used by the mode cards. */
export interface EffectInfo {
  mode: KeyboardMode;
  /** Dot colour shown on the card. */
  dot: string;
  /** Whether the effect animates (the preview should then run a clock). */
  animated: boolean;
}

/** Per-effect accent dot colours for the mode cards. */
const DOT: Record<KeyboardMode, string> = {
  off: "#64748b",
  static: "#22d3ee",
  breath: "#34d399",
  cycle: "#a78bfa",
  wave: "#38bdf8",
  dance: "#fbbf24",
  tempo: "#fb7185",
  flash: "#f8fafc",
  random: "#f472b6",
};

const BLACK: RGB = [0, 0, 0];

/** Whether an effect changes over time (and so needs an animation clock). */
export function isAnimated(mode: KeyboardMode): boolean {
  return mode !== "off" && mode !== "static";
}

/** The card metadata for a mode. */
export function effectInfo(mode: KeyboardMode): EffectInfo {
  return {
    mode,
    dot: DOT[mode] ?? "#94a3b8",
    animated: isAnimated(mode),
  };
}

/** Smooth 0..1 breathing curve, eased so the low end lingers. */
function breathe(t: number): number {
  const wave = (Math.sin(t) + 1) / 2;
  return 0.12 + Math.pow(wave, 1.8) * 0.88;
}

/**
 * Compute the preview frame for an effect at a point in time.
 *
 * `seconds` is a continuously increasing clock supplied by the caller; `base` is
 * the user's chosen colour. Effects that cycle through the spectrum ignore
 * `base` (the firmware does too).
 */
export function effectFrame(mode: KeyboardMode, base: RGB, seconds: number): EffectFrame {
  switch (mode) {
    case "off":
      return { rgb: BLACK, intensity: 0 };
    case "static":
      return { rgb: base, intensity: 1 };
    case "breath":
      return { rgb: base, intensity: breathe(seconds * 1.8) };
    case "cycle":
      // Whole zone walks the hue wheel together.
      return { rgb: hsvToRgb({ h: ((seconds * 0.12) % 1) * 360, s: 0.9, v: 1 }), intensity: 1 };
    case "wave": {
      // A moving band of light: the zone brightness sweeps up and down.
      const phase = (Math.sin(seconds * 2) + 1) / 2;
      return { rgb: base, intensity: 0.35 + phase * 0.65 };
    }
    case "dance":
      // Fast, energetic pulsing.
      return { rgb: base, intensity: (Math.sin(seconds * 6) + 1) / 2 };
    case "tempo": {
      // Sharp beat-like pulse with a short rest.
      const beat = seconds * 2.4;
      const frac = beat - Math.floor(beat);
      return { rgb: base, intensity: frac < 0.5 ? 1 - frac * 1.6 : 0.2 };
    }
    case "flash": {
      // Occasional strobe; mostly dark so the eye reads a flash, not a glow.
      const strobe = seconds * 5;
      const frac = strobe - Math.floor(strobe);
      return { rgb: base, intensity: frac < 0.18 ? 1 : 0 };
    }
    case "random": {
      // Deterministic per-half-second colour jump keeps the frame testable.
      const slot = Math.floor(seconds * 2);
      const hue = (slot * 0.61803398875) % 1;
      return { rgb: hsvToRgb({ h: hue * 360, s: 0.85, v: 1 }), intensity: 1 };
    }
    default:
      return { rgb: base, intensity: 1 };
  }
}

/** `rgba(...)` string for a frame, scaled by a caller alpha and the intensity. */
export function frameRgba(frame: EffectFrame, alpha = 1): string {
  const a = Math.max(0, Math.min(1, alpha * frame.intensity));
  const [r, g, b] = frame.rgb;
  return `rgba(${r}, ${g}, ${b}, ${a.toFixed(3)})`;
}
