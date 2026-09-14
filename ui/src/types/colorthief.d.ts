// colorthief 2.6 ships no type declarations. Its default export is a class with
// `getColor` (dominant colour) and `getPalette` (N-colour palette), both
// synchronous in the browser build.
declare module "colorthief" {
  type RGB = [number, number, number];
  class ColorThief {
    getColor(image: HTMLImageElement, quality?: number): RGB;
    getPalette(image: HTMLImageElement, colorCount?: number, quality?: number): RGB[];
  }
  export default ColorThief;
}
