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
