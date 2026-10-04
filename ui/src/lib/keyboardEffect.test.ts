import { describe, expect, it } from "vitest";
import { effectFrame, effectInfo } from "./keyboardEffect";
import { KEYBOARD_LAYOUT } from "./keyboardLayout";

const BASE: [number, number, number] = [255, 0, 0];

describe("keyboardEffect", () => {
  it("keeps the chosen colour for the static effect", () => {
    expect(effectFrame("static", BASE)).toEqual({ rgb: BASE, intensity: 1 });
    expect(effectInfo("static").dot).toBe("#22d3ee");
  });

  it("darkens fully when the effect is off", () => {
    expect(effectFrame("off", BASE)).toEqual({ rgb: [0, 0, 0], intensity: 0 });
    expect(effectInfo("off").dot).toBe("#64748b");
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
