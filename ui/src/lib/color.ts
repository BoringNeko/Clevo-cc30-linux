import ColorThief from "colorthief";

export type RGB = [number, number, number];

export interface ExtractedPalette {
  primary: RGB;
  secondary: RGB;
  swatches: RGB[];
}

const FALLBACK: ExtractedPalette = {
  primary: [120, 200, 255],
  secondary: [180, 120, 255],
  swatches: [
    [120, 200, 255],
    [180, 120, 255],
    [255, 150, 190],
    [140, 255, 210],
  ],
};

function clampChannel(v: number) {
  return Math.max(0, Math.min(255, Math.round(v)));
}

function toRgb(color: number[]): RGB {
  return [clampChannel(color[0]), clampChannel(color[1]), clampChannel(color[2])];
}

function saturation([r, g, b]: RGB) {
  const max = Math.max(r, g, b) / 255;
  const min = Math.min(r, g, b) / 255;
  const l = (max + min) / 2;
  if (max === min) return 0;
  return l > 0.5 ? (max - min) / (2 - max - min) : (max - min) / (max + min);
}

function luminance([r, g, b]: RGB) {
  return (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
}

/** Vivid enough to use as an accent, and light enough to read on dark glass. */
function isAccentWorthy(color: RGB) {
  const lum = luminance(color);
  return saturation(color) > 0.18 && lum > 0.22 && lum < 0.9;
}

/** Push a color toward a target lightness so accents stay legible on glass. */
export function normalizeAccent(color: RGB, targetLum = 0.62): RGB {
  const lum = luminance(color);
  if (lum <= 0.001) return color;
  const factor = targetLum / lum;
  return [clampChannel(color[0] * factor), clampChannel(color[1] * factor), clampChannel(color[2] * factor)];
}

export function rgbString(color: RGB, alpha = 1) {
  return alpha >= 1
    ? `rgb(${color[0]}, ${color[1]}, ${color[2]})`
    : `rgba(${color[0]}, ${color[1]}, ${color[2]}, ${alpha})`;
}

export function cssTriplet(color: RGB) {
  return `${color[0]} ${color[1]} ${color[2]}`;
}

/** Parse a user-entered `#rrggbb` (leading `#` optional) into an RGB tuple. */
export function hexToRgbTuple(hex: string): RGB | null {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return null;
  const n = parseInt(m[1], 16);
  return [(n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff];
}

/** `[r,g,b]` to lowercase `#rrggbb`. */
export function rgbTupleToHex([r, g, b]: RGB): string {
  const h = (n: number) => clampChannel(n).toString(16).padStart(2, "0");
  return `#${h(r)}${h(g)}${h(b)}`;
}

/** HSV with hue in degrees `0..360`, saturation/value in `0..1`. */
export interface HSV {
  h: number;
  s: number;
  v: number;
}

export function rgbToHsv([r, g, b]: RGB): HSV {
  const rn = r / 255;
  const gn = g / 255;
  const bn = b / 255;
  const max = Math.max(rn, gn, bn);
  const min = Math.min(rn, gn, bn);
  const d = max - min;
  let h = 0;
  if (d !== 0) {
    if (max === rn) h = ((gn - bn) / d) % 6;
    else if (max === gn) h = (bn - rn) / d + 2;
    else h = (rn - gn) / d + 4;
    h *= 60;
    if (h < 0) h += 360;
  }
  return { h, s: max === 0 ? 0 : d / max, v: max };
}

export function hsvToRgb({ h, s, v }: HSV): RGB {
  const c = v * s;
  const hp = (((h % 360) + 360) % 360) / 60;
  const x = c * (1 - Math.abs((hp % 2) - 1));
  let rgb: [number, number, number];
  if (hp < 1) rgb = [c, x, 0];
  else if (hp < 2) rgb = [x, c, 0];
  else if (hp < 3) rgb = [0, c, x];
  else if (hp < 4) rgb = [0, x, c];
  else if (hp < 5) rgb = [x, 0, c];
  else rgb = [c, 0, x];
  const m = v - c;
  return [
    clampChannel((rgb[0] + m) * 255),
    clampChannel((rgb[1] + m) * 255),
    clampChannel((rgb[2] + m) * 255),
  ];
}

function rgbToHsl([r, g, b]: RGB): [number, number, number] {
  const rn = r / 255;
  const gn = g / 255;
  const bn = b / 255;
  const max = Math.max(rn, gn, bn);
  const min = Math.min(rn, gn, bn);
  const l = (max + min) / 2;
  if (max === min) return [0, 0, l];
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  let h: number;
  if (max === rn) h = (gn - bn) / d + (gn < bn ? 6 : 0);
  else if (max === gn) h = (bn - rn) / d + 2;
  else h = (rn - gn) / d + 4;
  return [h / 6, s, l];
}

function hslToRgb([h, s, l]: [number, number, number]): RGB {
  const hue = (p: number, q: number, t: number) => {
    let tt = t;
    if (tt < 0) tt += 1;
    if (tt > 1) tt -= 1;
    if (tt < 1 / 6) return p + (q - p) * 6 * tt;
    if (tt < 1 / 2) return q;
    if (tt < 2 / 3) return p + (q - p) * (2 / 3 - tt) * 6;
    return p;
  };
  if (s === 0) {
    const v = clampChannel(l * 255);
    return [v, v, v];
  }
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
  const p = 2 * l - q;
  return [
    clampChannel(hue(p, q, h + 1 / 3) * 255),
    clampChannel(hue(p, q, h) * 255),
    clampChannel(hue(p, q, h - 1 / 3) * 255),
  ];
}

/** Rotate a colour's hue by `deg`, keeping saturation and lightness. */
export function rotateHue(color: RGB, deg: number): RGB {
  const [h, s, l] = rgbToHsl(color);
  return hslToRgb([((h + deg / 360) % 1 + 1) % 1, s, l]);
}

/**
 * Apply the user's accent override on top of the wallpaper palette.
 *
 * Components consume `palette.primary` / `palette.secondary` for the gauge,
 * curves, charts, sidebar and buttons, so the override has to live in the
 * palette itself — not only in the MUI theme — or those surfaces stay on the
 * wallpaper colour. The secondary is derived from the chosen accent by a hue
 * rotation (rather than kept from the wallpaper) so both chart series follow the
 * override while remaining visually distinct.
 */
export function withAccent(
  palette: ExtractedPalette,
  accent: string | null,
): ExtractedPalette {
  const override = accent ? hexToRgbTuple(accent) : null;
  if (!override) return palette;

  const rotated = rotateHue(override, 45);
  // A grey accent has no hue to rotate, so rotation is a no-op and both series
  // would look identical; keep the wallpaper's secondary in that case.
  const secondary =
    colorDistance(override, rotated) < 12
      ? palette.secondary
      : normalizeAccent(rotated, 0.66);

  return { ...palette, primary: override, secondary };
}

export function colorDistance(a: RGB, b: RGB) {
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}

/**
 * Extract a dominant + secondary palette from an already-loaded image element.
 * Falls back gracefully when the image is flat / monochrome.
 */
export function extractPalette(img: HTMLImageElement): ExtractedPalette {
  try {
    const thief = new ColorThief();
    const dominant = toRgb(thief.getColor(img) as number[]);
    const rawSwatches = (thief.getPalette(img, 8) as number[][]).map(toRgb);

    const accentCandidates = rawSwatches.filter(isAccentWorthy);
    const primarySource =
      (isAccentWorthy(dominant) ? dominant : accentCandidates[0]) ?? dominant;

    const secondarySource =
      accentCandidates.find((c) => colorDistance(c, primarySource) > 60) ??
      rawSwatches.find((c) => colorDistance(c, primarySource) > 60) ??
      primarySource;

    const primary = normalizeAccent(primarySource, 0.62);
    const secondary = normalizeAccent(secondarySource, 0.66);

    const swatches = [primary, secondary, ...rawSwatches]
      .filter(
        (c, i, arr) =>
          arr.findIndex((o) => colorDistance(o, c) < 24) === i
      )
      .slice(0, 5);

    return { primary, secondary, swatches: swatches.length ? swatches : FALLBACK.swatches };
  } catch {
    return FALLBACK;
  }
}

export { FALLBACK as FALLBACK_PALETTE };
