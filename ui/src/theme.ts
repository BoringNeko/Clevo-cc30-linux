import { createTheme, type Theme } from "@mui/material/styles";
import type { ExtractedPalette } from "./lib/color";

/**
 * Appearance settings chosen in Settings, layered on top of the wallpaper
 * palette.
 *
 * - `mode`: dark or light glass.
 * - `accent`: override the wallpaper-derived accent colour (hex, or null to use
 *   the wallpaper's).
 * - `surface`: override the glass surface colour (hex), or null for the
 *   mode-appropriate default.
 * - `textColor`: override the primary text colour (hex), or null.
 */
export interface Appearance {
  mode: "dark" | "light";
  accent: string | null;
  surface: string | null;
  textColor: string | null;
  /** Glass opacity override (0..1); null uses the mode default. */
  opacity: number | null;
  /** Blur radius in px (0..40); ignored when the blur switch is off. */
  blurPx: number;
  /** Brand text; empty string hides it. */
  brandTitle: string;
  brandSubtitle: string;
  /**
   * Default logo image: a path relative to the app root (served from
   * `public/`, e.g. "logo.jpg") or an absolute filesystem path (starting with
   * "/") read via the backend. Empty uses the built-in icon.
   */
  logoPath: string;
  /** Window aspect ratio preset. */
  /** Window aspect ratio; selects which resolution list is shown. */
  aspect: AspectRatio;
  /** Window width in px; always the width of a preset for the current aspect. */
  displayWidth: number;
  /** Window height in px; always the height of a preset for the current aspect. */
  displayHeight: number;
  /** UI zoom factor (0.5..2), applied to the whole interface. */
  scale: number;
}

export type AspectRatio = "16:9" | "16:10";

/**
 * The layout is designed against this logical viewport. Every fixed size in the
 * UI (card padding, gauge diameter, sidebar width) assumes it, and the whole
 * interface is then scaled uniformly to the actual viewport, so the design fits
 * any window without ever scrolling.
 *
 * The width is fixed; the height follows the chosen aspect ratio, so a 16:10
 * window fills its full height instead of showing black bands above and below a
 * 16:9 surface.
 */
export const DESIGN_WIDTH = 1600;
export const DESIGN_HEIGHT = 900;

/** The design surface size for an aspect ratio: always 1600 wide. */
export function designSize(aspect: AspectRatio): { width: number; height: number } {
  return {
    width: DESIGN_WIDTH,
    height: aspect === "16:10" ? 1000 : DESIGN_HEIGHT,
  };
}

export const ASPECT_RATIOS: readonly AspectRatio[] = ["16:9", "16:10"];

/**
 * Upper bound on the design scale. The 4K presets need 2.4 (3840/1600), so this
 * has to sit above that or those windows would letterbox.
 */
export const MAX_DESIGN_SCALE = 4;

/**
 * Preset window resolutions per aspect ratio, smallest to largest. Stored as
 * exact integers so odd sizes like 854×480 stay exact.
 */
export const RESOLUTION_PRESETS: Record<
  AspectRatio,
  ReadonlyArray<{ width: number; height: number }>
> = {
  "16:9": [
    { width: 568, height: 320 },
    { width: 854, height: 480 },
    { width: 1024, height: 576 },
    { width: 1280, height: 720 },
    { width: 1600, height: 900 },
    { width: 1920, height: 1080 },
    { width: 2560, height: 1440 },
    { width: 3840, height: 2160 },
  ],
  "16:10": [
    { width: 1280, height: 800 },
    { width: 1440, height: 900 },
    { width: 1680, height: 1050 },
    { width: 1920, height: 1200 },
    { width: 2560, height: 1600 },
    { width: 3840, height: 2400 },
  ],
};

export const SCALE_MIN = 50;
export const SCALE_MAX = 200;

export const DEFAULT_APPEARANCE: Appearance = {
  mode: "dark",
  accent: null,
  surface: null,
  textColor: null,
  opacity: null,
  blurPx: 24,
  brandTitle: "CLEVO",
  brandSubtitle: "CONTROL",
  logoPath: "logo.jpg",
  aspect: "16:9",
  displayWidth: DESIGN_WIDTH,
  displayHeight: DESIGN_HEIGHT,
  scale: 100,
};

/**
 * The preferred preset for a ratio: the one matching the design size, else the
 * closest to it.
 */
export function defaultSize(aspect: AspectRatio): { width: number; height: number } {
  const presets = RESOLUTION_PRESETS[aspect];
  const exact = presets.find(
    (p) => p.width === DESIGN_WIDTH && p.height === DESIGN_HEIGHT,
  );
  if (exact) return { width: exact.width, height: exact.height };
  const preset = presets.reduce((best, p) =>
    Math.abs(p.width - DESIGN_WIDTH) < Math.abs(best.width - DESIGN_WIDTH) ? p : best,
  );
  return { width: preset.width, height: preset.height };
}

/**
 * The chosen window size, snapped to a preset of the current ratio: an exact
 * match if present, else the closest height, else the ratio's default.
 */
export function windowSize(appearance: Appearance): { width: number; height: number } {
  const presets = RESOLUTION_PRESETS[appearance.aspect];
  const preset =
    presets.find((p) => p.width === appearance.displayWidth && p.height === appearance.displayHeight) ??
    presets.find((p) => p.height === appearance.displayHeight) ??
    (() => {
      const def = defaultSize(appearance.aspect);
      return presets.find((p) => p.width === def.width && p.height === def.height)!;
    })();
  return { width: preset.width, height: preset.height };
}

/**
 * The factor that maps the design space onto the given viewport.
 *
 * `min` (not `max`) so the whole design is always visible: with an unexpected
 * aspect ratio the interface is letterboxed rather than cropped. Because the
 * design surface itself follows the chosen ratio (`designSize`), a 16:10 window
 * normally fits exactly and shows no letterboxing at all.
 *
 * The ceiling is 4 so the 4K presets (3840 wide, needing 2.4) still fill the
 * window; a lower cap would leave black borders on a large display. The floor
 * keeps a tiny window usable.
 */
export function designScale(
  viewportWidth: number,
  viewportHeight: number,
  designWidth: number = DESIGN_WIDTH,
  designHeight: number = DESIGN_HEIGHT,
): number {
  if (viewportWidth <= 0 || viewportHeight <= 0) return 1;
  if (designWidth <= 0 || designHeight <= 0) return 1;
  const scale = Math.min(viewportWidth / designWidth, viewportHeight / designHeight);
  return Math.min(MAX_DESIGN_SCALE, Math.max(0.25, scale));
}

/** Hex `#rrggbb` to an `r, g, b` string, or null when malformed. */
function hexToRgb(hex: string): string | null {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return null;
  const n = parseInt(m[1], 16);
  return `${(n >> 16) & 0xff}, ${(n >> 8) & 0xff}, ${n & 0xff}`;
}

/**
 * Resolve the surface RGBA for a mode. The surface is the glass base; when the
 * user picked a custom colour we keep the same opacity scheme so the blur still
 * reads as glass.
 */
function surfaceColor(appearance: Appearance, blur: boolean): string {
  const defaultOpacity = blur ? (appearance.mode === "dark" ? 0.42 : 0.55) : 0.92;
  const opacity = appearance.opacity ?? defaultOpacity;
  const custom = appearance.surface ? hexToRgb(appearance.surface) : null;
  if (custom) return `rgba(${custom}, ${opacity})`;
  return appearance.mode === "dark"
    ? `rgba(0, 0, 0, ${opacity})`
    : `rgba(255, 255, 255, ${opacity})`;
}

/** Blur style for a given appearance and enabled flag. */
function blurStyleFor(appearance: Appearance, blur: boolean) {
  if (!blur) return {};
  return {
    backdropFilter: `blur(${appearance.blurPx}px) saturate(140%)`,
    WebkitBackdropFilter: `blur(${appearance.blurPx}px) saturate(140%)`,
  };
}

/** Foreground/text colours for the mode (or the user's override). */
function textColors(appearance: Appearance) {
  const custom = appearance.textColor ? hexToRgb(appearance.textColor) : null;
  if (custom) {
    const base = `rgb(${custom})`;
    return {
      primary: base,
      secondary: `rgba(${custom}, 0.72)`,
      muted: `rgba(${custom}, 0.52)`,
      faint: `rgba(${custom}, 0.38)`,
      border: `rgba(${custom}, 0.18)`,
    };
  }
  if (appearance.mode === "dark") {
    return {
      primary: "#ffffff",
      secondary: "rgba(255,255,255,0.72)",
      muted: "rgba(255,255,255,0.55)",
      faint: "rgba(255,255,255,0.40)",
      border: "rgba(255,255,255,0.20)",
    };
  }
  return {
    primary: "#111417",
    secondary: "rgba(20,24,28,0.78)",
    muted: "rgba(20,24,28,0.60)",
    faint: "rgba(20,24,28,0.45)",
    border: "rgba(20,24,28,0.16)",
  };
}

/** Build the MUI theme from the wallpaper palette, appearance and blur flag. */
export function buildTheme(
  palette: ExtractedPalette,
  blur = true,
  appearance: Appearance = DEFAULT_APPEARANCE,
): Theme {
  // The caller passes a palette that already has the accent override applied
  // (see `withAccent`), so the theme and the components agree on one colour.
  const { primary, secondary } = palette;
  const primaryMain = `rgb(${primary[0]}, ${primary[1]}, ${primary[2]})`;
  const secondaryMain = `rgb(${secondary[0]}, ${secondary[1]}, ${secondary[2]})`;

  const surface = surfaceColor(appearance, blur);
  const text = textColors(appearance);
  const blurStyle = blurStyleFor(appearance, blur);

  return createTheme({
    palette: {
      mode: appearance.mode,
      primary: { main: primaryMain },
      secondary: { main: secondaryMain },
      background: { default: "transparent", paper: "transparent" },
      text: { primary: text.primary, secondary: text.secondary },
      divider: text.border,
      success: { main: "#4ade80" },
      warning: { main: "#fbbf24" },
      error: { main: "#f87171" },
      action: {
        // Subtle fills for icon backgrounds, hover states and rails. These are
        // mode-aware so light mode does not paint white-on-white.
        hover: appearance.mode === "dark" ? "rgba(255,255,255,0.08)" : "rgba(0,0,0,0.05)",
        selected: appearance.mode === "dark" ? "rgba(255,255,255,0.12)" : "rgba(0,0,0,0.08)",
        disabled: text.faint,
        active: text.secondary,
      },
    },
    shape: { borderRadius: 8 },
    typography: {
      fontFamily: 'Inter, system-ui, "Segoe UI", Roboto, sans-serif',
      button: { textTransform: "none", fontWeight: 600 },
    },
    components: {
      MuiCssBaseline: {
        styleOverrides: { body: { transition: "background-color 500ms ease" } },
      },
      MuiCard: {
        defaultProps: { elevation: 0 },
        styleOverrides: {
          root: {
            backgroundColor: surface,
            ...blurStyle,
            border: `1px solid ${text.border}`,
            borderRadius: 8,
            boxShadow:
              appearance.mode === "dark"
                ? "0 10px 30px rgba(0,0,0,0.35)"
                : "0 10px 30px rgba(0,0,0,0.12)",
            isolation: "isolate",
            transform: "translateZ(0)",
            transition: "border-color 500ms ease, background-color 500ms ease",
          },
        },
      },
      MuiButton: {
        defaultProps: { disableElevation: true },
        styleOverrides: { root: { borderRadius: 6, transition: "all 300ms ease" } },
      },
      MuiTooltip: {
        styleOverrides: {
          tooltip: {
            backgroundColor: appearance.mode === "dark" ? "rgba(0,0,0,0.8)" : "rgba(255,255,255,0.95)",
            backdropFilter: "blur(8px)",
            border: `1px solid ${text.border}`,
            borderRadius: 6,
            fontSize: "0.7rem",
            color: text.primary,
          },
        },
      },
      MuiSelect: { styleOverrides: { root: { borderRadius: 6 } } },
      MuiPopover: {
        defaultProps: { disableScrollLock: true },
        styleOverrides: {
          paper: {
            backgroundColor: surface,
            ...blurStyle,
            backgroundImage: "none",
            border: `1px solid ${text.border}`,
            borderRadius: 8,
            boxShadow:
              appearance.mode === "dark"
                ? "0 10px 30px rgba(0,0,0,0.35)"
                : "0 10px 30px rgba(0,0,0,0.12)",
          },
        },
      },
    },
  });
}

/**
 * Shared glass surface for non-Card containers (sidebar, header). Uses the same
 * colour as cards so headers and cards do not differ in depth.
 */
export function glassSx(
  blur = true,
  appearance: Appearance = DEFAULT_APPEARANCE,
) {
  const text = textColors(appearance);
  return {
    backgroundColor: surfaceColor(appearance, blur),
    ...blurStyleFor(appearance, blur),
    border: `1px solid ${text.border}`,
    borderRadius: 1,
    boxShadow:
      appearance.mode === "dark"
        ? "0 10px 30px rgba(0,0,0,0.35)"
        : "0 10px 30px rgba(0,0,0,0.12)",
    isolation: "isolate",
    transform: "translateZ(0)",
    transition: "border-color 500ms ease, background-color 500ms ease",
  } as const;
}

/** Semantic status colours, fixed regardless of the wallpaper. */
export const statusColors = {
  good: {
    color: "#4ade80",
    backgroundColor: "rgba(74,222,128,0.12)",
    borderColor: "rgba(74,222,128,0.28)",
  },
  warn: {
    color: "#fbbf24",
    backgroundColor: "rgba(251,191,36,0.14)",
    borderColor: "rgba(251,191,36,0.30)",
  },
  bad: {
    color: "#f87171",
    backgroundColor: "rgba(248,113,113,0.14)",
    borderColor: "rgba(248,113,113,0.30)",
  },
  neutral: {
    color: "rgba(127,127,127,0.95)",
    backgroundColor: "rgba(127,127,127,0.14)",
    borderColor: "rgba(127,127,127,0.30)",
  },
} as const;

/** Map a freshness value to a status tone. */
export function freshnessTone(freshness: string): keyof typeof statusColors {
  switch (freshness) {
    case "fresh":
      return "good";
    case "stale":
      return "warn";
    default:
      return "neutral";
  }
}
