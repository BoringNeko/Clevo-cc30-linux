import { describe, expect, it } from "vitest";

import AppSource from "./App.tsx?raw";

/**
 * The interface is laid out at a fixed design surface — 1600x900 for 16:9,
 * 1600x1000 for 16:10 — and scaled to the window, so MUI's viewport breakpoints
 * are wrong here: on a window whose logical viewport is under the breakpoint, a
 * responsive rule would collapse a grid and roughly double the content height,
 * which brings back the scrollbar.
 *
 * Every source file is pulled in with Vite's `?raw` glob so the check needs no
 * filesystem APIs.
 */
const SOURCES = import.meta.glob("./**/*.{ts,tsx}", {
  query: "?raw",
  import: "default",
  eager: true,
  // The check is about component styling, not the tests themselves.
  // (Filtered below as well, because glob negation needs a leading "!".)
}) as Record<string, string>;

/** Breakpoint keys MUI resolves against the viewport width. */
const BREAKPOINT = /\b(xs|sm|md|lg|xl):/g;

describe("design-surface layout", () => {
  it("no component uses viewport breakpoints in sx props", () => {
    const offenders: string[] = [];

    for (const [path, text] of Object.entries(SOURCES)) {
      if (/\.test\.tsx?$/.test(path)) continue;
      for (const match of text.matchAll(BREAKPOINT)) {
        const line = text.slice(0, match.index).split("\n").length;
        offenders.push(`${path.replace(/^\.\//, "")}:${line} (${match[0]})`);
      }
    }

    expect(offenders).toEqual([]);
  });

  it("the design surface size follows the aspect ratio", () => {
    // The surface must not be hard-coded to 1600x900, or a 16:10 window would
    // letterbox; it comes from the aspect-aware design size.
    expect(AppSource).toContain("designWidth");
    expect(AppSource).toContain("designHeight");
    expect(AppSource).toContain("useDesignScale(appearance.aspect)");
    expect(AppSource).toContain("designScaleFactor");
  });

  it("the dashboard grid uses fixed rows so it cannot outgrow the surface", () => {
    // `minmax(0, 1fr)` rather than plain `1fr`: a plain `1fr` row cannot shrink
    // below its content's intrinsic height, so the two card rows would overflow
    // the surface and eat the bottom margin.
    expect(AppSource).toContain(
      'gridTemplateRows: "minmax(0, 1fr) minmax(0, 1fr)"',
    );
  });
});
