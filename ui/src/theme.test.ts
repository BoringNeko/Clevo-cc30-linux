import { describe, expect, it } from "vitest";
import {
  buildTheme,
  DEFAULT_APPEARANCE,
  defaultSize,
  designScale,
  designSize,
  DESIGN_HEIGHT,
  DESIGN_WIDTH,
  freshnessTone,
  RESOLUTION_PRESETS,
  statusColors,
  type Appearance,
} from "./theme";
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

describe("design basis", () => {
  it("the design size is 1600x900", () => {
    expect(DESIGN_WIDTH).toBe(1600);
    expect(DESIGN_HEIGHT).toBe(900);
  });

  it("defaults to the design size", () => {
    expect(DEFAULT_APPEARANCE.displayWidth).toBe(DESIGN_WIDTH);
    expect(DEFAULT_APPEARANCE.displayHeight).toBe(DESIGN_HEIGHT);
  });

  it("the 16:9 presets include the design size", () => {
    const found = RESOLUTION_PRESETS["16:9"].some(
      (p) => p.width === DESIGN_WIDTH && p.height === DESIGN_HEIGHT,
    );
    expect(found).toBe(true);
  });

  it("defaultSize prefers the design size", () => {
    expect(defaultSize("16:9")).toEqual({ width: 1600, height: 900 });
  });

  it("scales to 1 at exactly the design size", () => {
    expect(designScale(DESIGN_WIDTH, DESIGN_HEIGHT)).toBe(1);
  });

  // The whole design must stay visible, so the smaller of the two ratios wins.
  it("fits the design inside a wider viewport", () => {
    // 3200x900: height is the constraint.
    expect(designScale(3200, 900)).toBeCloseTo(1);
  });

  it("fits the design inside a taller viewport", () => {
    // 1600x1800: width is the constraint.
    expect(designScale(1600, 1800)).toBeCloseTo(1);
  });

  it("shrinks for a smaller viewport", () => {
    // The measured case: a 1600x900 window under Xft.dpi=129 gives 1185x666.
    const s = designScale(1185, 666);
    expect(s).toBeLessThan(1);
    // min(1185/1600, 666/900) = min(0.7406, 0.74)
    expect(s).toBeCloseTo(0.74, 2);
    // The scaled design still fits inside the viewport.
    expect(DESIGN_WIDTH * s).toBeLessThanOrEqual(1185 + 0.5);
    expect(DESIGN_HEIGHT * s).toBeLessThanOrEqual(666 + 0.5);
  });

  it("clamps degenerate viewports", () => {
    expect(designScale(0, 0)).toBe(1);
    expect(designScale(-10, 100)).toBe(1);
    expect(designScale(10, 10)).toBeGreaterThan(0);
  });
});

describe("design surface per aspect ratio", () => {
  it("is 1600x900 for 16:9", () => {
    expect(designSize("16:9")).toEqual({ width: 1600, height: 900 });
  });

  it("is 1600x1000 for 16:10", () => {
    expect(designSize("16:10")).toEqual({ width: 1600, height: 1000 });
  });

  // A 16:10 window is taller than 16:9, so a 16:9 surface would scale to the
  // width and leave black bands above and below. The surface has to follow the
  // ratio for the window to be filled edge to edge.
  it.each(RESOLUTION_PRESETS["16:10"].map((p) => [p.width, p.height] as const))(
    "fills a %ix%i 16:10 window with no black border",
    (width, height) => {
      const design = designSize("16:10");
      const s = designScale(width, height, design.width, design.height);

      // The scaled surface covers the viewport on both axes.
      expect(design.width * s).toBeCloseTo(width, 5);
      expect(design.height * s).toBeCloseTo(height, 5);
    },
  );

  it.each(RESOLUTION_PRESETS["16:9"].map((p) => [p.width, p.height] as const))(
    "fills a %ix%i 16:9 window with no black border",
    (width, height) => {
      const design = designSize("16:9");
      const s = designScale(width, height, design.width, design.height);

      // Exact for true 16:9 sizes; a couple of the small presets (568x320,
      // 854x480) are only approximately 16:9, so allow a sub-pixel remainder
      // rather than a visible band.
      expect(design.width * s).toBeGreaterThanOrEqual(width - 1);
      expect(design.height * s).toBeGreaterThanOrEqual(height - 1);
      expect(design.width * s).toBeLessThanOrEqual(width + 0.5);
      expect(design.height * s).toBeLessThanOrEqual(height + 0.5);
    },
  );

  // The regression this guards: with the fixed 16:9 surface a 16:10 window left
  // bars of `height - 900 * scale`.
  it("would letterbox a 16:10 window if the surface stayed 16:9", () => {
    const s = designScale(1920, 1200, DESIGN_WIDTH, DESIGN_HEIGHT);
    const bars = 1200 - DESIGN_HEIGHT * s;
    expect(bars).toBeGreaterThan(0);

    // Following the ratio removes them.
    const fixed = designSize("16:10");
    const s2 = designScale(1920, 1200, fixed.width, fixed.height);
    expect(1200 - fixed.height * s2).toBeCloseTo(0, 5);
  });
});
