import { describe, expect, it } from "vitest";
import { buildTheme, DEFAULT_APPEARANCE, freshnessTone, statusColors, type Appearance } from "./theme";
import { FALLBACK_PALETTE, withAccent } from "./lib/color";

describe("buildTheme", () => {
  it("derives the accent colours from the palette", () => {
    const theme = buildTheme({
      primary: [10, 20, 30],
      secondary: [40, 50, 60],
      swatches: [[10, 20, 30]],
    });
    expect(theme.palette.primary.main).toBe("rgb(10, 20, 30)");
    expect(theme.palette.secondary.main).toBe("rgb(40, 50, 60)");
    expect(theme.palette.mode).toBe("dark");
  });

  it("builds from the fallback palette without error", () => {
    const theme = buildTheme(FALLBACK_PALETTE);
    expect(theme.palette.primary.main).toMatch(/^rgb\(/);
  });

  it("uses the fixed glass radius (8px)", () => {
    expect(buildTheme(FALLBACK_PALETTE).shape.borderRadius).toBe(8);
  });
});

describe("freshnessTone", () => {
  it("maps freshness to a semantic tone", () => {
    expect(freshnessTone("fresh")).toBe("good");
    expect(freshnessTone("stale")).toBe("warn");
    expect(freshnessTone("unknown")).toBe("neutral");
  });

  it("every tone has a colour", () => {
    for (const tone of ["good", "warn", "bad", "neutral"] as const) {
      expect(statusColors[tone].color).toBeTruthy();
    }
  });
});

describe("appearance", () => {
  const app = (patch: Partial<Appearance>): Appearance => ({ ...DEFAULT_APPEARANCE, ...patch });

  it("light mode uses a light surface and dark text", () => {
    const theme = buildTheme(FALLBACK_PALETTE, true, app({ mode: "light" }));
    expect(theme.palette.mode).toBe("light");
    expect(theme.palette.text.primary).toBe("#111417");
  });

  it("a custom accent reaches the theme via the overridden palette", () => {
    // App applies the override with `withAccent` before building the theme, so
    // one colour covers the theme and every palette-consuming component.
    const palette = withAccent(FALLBACK_PALETTE, "#ff0000");
    const theme = buildTheme(palette, true, app({ accent: "#ff0000" }));
    expect(theme.palette.primary.main).toBe("rgb(255, 0, 0)");
  });

  it("a custom surface tints the card background", () => {
    const theme = buildTheme(FALLBACK_PALETTE, true, app({ surface: "#0000ff" }));
    const card = theme.components?.MuiCard?.styleOverrides?.root as { backgroundColor: string };
    expect(card.backgroundColor).toMatch(/^rgba\(0, 0, 255,/);
  });

  it("a custom text colour applies to the palette", () => {
    const theme = buildTheme(FALLBACK_PALETTE, true, app({ textColor: "#00ff00" }));
    expect(theme.palette.text.primary).toBe("rgb(0, 255, 0)");
  });

  it("a custom opacity overrides the card alpha", () => {
    const theme = buildTheme(FALLBACK_PALETTE, true, app({ opacity: 0.8 }));
    const card = theme.components?.MuiCard?.styleOverrides?.root as { backgroundColor: string };
    expect(card.backgroundColor).toMatch(/0\.8\)$/);
  });

  it("a custom blur radius reaches the card backdrop filter", () => {
    const theme = buildTheme(FALLBACK_PALETTE, true, app({ blurPx: 8 }));
    const card = theme.components?.MuiCard?.styleOverrides?.root as { backdropFilter: string };
    expect(card.backdropFilter).toContain("blur(8px)");
  });
});
