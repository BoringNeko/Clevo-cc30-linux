import { describe, expect, it } from "vitest";
import {
  colorDistance,
  FALLBACK_PALETTE,
  hexToRgbTuple,
  rotateHue,
  withAccent,
} from "./color";

describe("withAccent", () => {
  it("applies an accent override to the primary colour", () => {
    const out = withAccent(FALLBACK_PALETTE, "#aa0000");
    expect(out.primary).toEqual([170, 0, 0]);
  });

  it("keeps the wallpaper palette when there is no override", () => {
    expect(withAccent(FALLBACK_PALETTE, null)).toBe(FALLBACK_PALETTE);
  });

  it("ignores a malformed override", () => {
    expect(withAccent(FALLBACK_PALETTE, "not-a-colour")).toBe(FALLBACK_PALETTE);
  });

  it("derives a distinct secondary from the accent so both series follow it", () => {
    const out = withAccent(FALLBACK_PALETTE, "#00ff00");
    expect(out.secondary).not.toEqual(FALLBACK_PALETTE.secondary);
    expect(out.secondary).not.toEqual(out.primary);
    expect(colorDistance(out.primary, out.secondary)).toBeGreaterThan(60);
  });

  it("falls back to the wallpaper secondary for a grey accent", () => {
    const out = withAccent(FALLBACK_PALETTE, "#808080");
    expect(out.primary).toEqual([128, 128, 128]);
    expect(out.secondary).toEqual(FALLBACK_PALETTE.secondary);
  });

  it("parses hex with and without a leading hash", () => {
    expect(hexToRgbTuple("#123456")).toEqual([18, 52, 86]);
    expect(hexToRgbTuple("123456")).toEqual([18, 52, 86]);
    expect(hexToRgbTuple("nope")).toBeNull();
  });
});

describe("rotateHue", () => {
  it("shifts a saturated colour to a different hue", () => {
    const out = rotateHue([255, 0, 0], 45);
    expect(colorDistance([255, 0, 0], out)).toBeGreaterThan(60);
  });

  it("is a no-op for greys", () => {
    expect(rotateHue([128, 128, 128], 45)).toEqual([128, 128, 128]);
  });
});
