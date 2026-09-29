import { describe, expect, it } from "vitest";
import { effectFrame, effectInfo, frameRgba, isAnimated } from "./keyboardEffect";
import { KEYBOARD_LAYOUT } from "./keyboardLayout";

const BASE: [number, number, number] = [255, 0, 0];

describe("keyboardEffect", () => {
  it("keeps the chosen colour for the static effect", () => {
    const frame = effectFrame("static", BASE, 12.3);
    expect(frame.rgb).toEqual(BASE);
    expect(frame.intensity).toBe(1);
  });

  it("darkens fully when the effect is off", () => {
    const frame = effectFrame("off", BASE, 3);
    expect(frame.rgb).toEqual([0, 0, 0]);
    expect(frame.intensity).toBe(0);
  });

  it("cycles the whole zone through the hue wheel", () => {
    const a = effectFrame("cycle", BASE, 0);
    const b = effectFrame("cycle", BASE, 3);
    // Spectrum effects ignore the base colour and move on their own.
    expect(a.rgb).not.toEqual(b.rgb);
    expect(a.intensity).toBe(1);
  });

  it("animates brightness without changing the hue for pulsing effects", () => {
    for (const mode of ["breath", "wave", "dance", "tempo"] as const) {
      const a = effectFrame(mode, BASE, 0.1);
      const b = effectFrame(mode, BASE, 0.4);
      expect(a.rgb).toEqual(BASE);
      expect(b.rgb).toEqual(BASE);
      expect(a.intensity).not.toBe(b.intensity);
    }
  });

  it("keeps every intensity inside 0..1", () => {
    for (const mode of ["breath", "cycle", "wave", "dance", "tempo", "flash", "random"] as const) {
      for (let t = 0; t < 4; t += 0.05) {
        const { intensity } = effectFrame(mode, BASE, t);
        expect(intensity).toBeGreaterThanOrEqual(0);
        expect(intensity).toBeLessThanOrEqual(1);
      }
    }
  });

  it("produces deterministic random frames so the preview is testable", () => {
    expect(effectFrame("random", BASE, 2.0)).toEqual(effectFrame("random", BASE, 2.4));
    expect(effectFrame("random", BASE, 2.0).rgb).not.toEqual(
      effectFrame("random", BASE, 2.6).rgb,
    );
  });

  it("marks only the static effects as non-animated", () => {
    expect(effectInfo("off").animated).toBe(false);
    expect(effectInfo("static").animated).toBe(false);
    // Even a mostly-dark strobe still changes over time.
    expect(effectInfo("flash").animated).toBe(true);
    expect(effectInfo("wave").animated).toBe(true);
    expect(isAnimated("random")).toBe(true);
    expect(isAnimated("static")).toBe(false);
  });

  it("scales rgba by the caller alpha and the frame intensity", () => {
    expect(frameRgba({ rgb: [10, 20, 30], intensity: 0.5 }, 0.4)).toBe("rgba(10, 20, 30, 0.200)");
  });
});

describe("KEYBOARD_LAYOUT", () => {
  it("has six rows that each span the full width", () => {
    expect(KEYBOARD_LAYOUT).toHaveLength(6);
    for (const row of KEYBOARD_LAYOUT) {
      const units = row.reduce((sum, key) => sum + key.w, 0);
      expect(units).toBeCloseTo(15, 5);
    }
  });

  it("uses unique key codes so a physical key maps to one cap", () => {
    const codes = KEYBOARD_LAYOUT.flat().map((key) => key.code);
    expect(new Set(codes).size).toBe(codes.length);
  });
});
