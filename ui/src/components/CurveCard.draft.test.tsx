import { describe, expect, it, vi } from "vitest";
import { render, fireEvent, screen } from "@testing-library/react";
import { CurveCard, sameCurve, shouldAdoptCurve } from "./CurveCard";
import type { CurvePoint, FanCurve } from "../api/daemon";

vi.mock("../api/daemon", async () => {
  const actual = await vi.importActual<typeof import("../api/daemon")>("../api/daemon");
  return { ...actual, setFanCurve: vi.fn().mockResolvedValue(undefined) };
});

const P = (temp: number, duty_pct: number): CurvePoint => ({ temp, duty_pct });

const BASE = [P(40, 25), P(60, 36), P(80, 53), P(100, 100)];
const EDITED = [P(40, 25), P(72, 80), P(80, 53), P(100, 100)];

describe("sameCurve", () => {
  it("compares by content, not identity", () => {
    expect(sameCurve(BASE, BASE.map((p) => ({ ...p })))).toBe(true);
  });

  it("detects a changed point", () => {
    expect(sameCurve(BASE, EDITED)).toBe(false);
  });

  it("detects a different length", () => {
    expect(sameCurve(BASE, BASE.slice(0, 3))).toBe(false);
  });
});

describe("shouldAdoptCurve", () => {
  it("adopts a genuinely new curve when the draft matches the daemon", () => {
    expect(shouldAdoptCurve(EDITED, BASE, BASE)).toBe(true);
  });

  it("ignores an equal curve that merely arrived as a new object", () => {
    // Every poll re-serialises the curve into a fresh array.
    expect(shouldAdoptCurve(BASE.map((p) => ({ ...p })), BASE, BASE)).toBe(false);
  });

  it("never discards an unsaved local edit", () => {
    // The draft has moved away from the daemon's values; an incoming change
    // must wait rather than overwrite the user's work.
    expect(shouldAdoptCurve(EDITED, BASE, EDITED)).toBe(false);
  });

  it("a curve arriving from elsewhere is not mistaken for a local edit", () => {
    // The regression: measuring "unsaved edit" against the *incoming* curve
    // made every external change look like a local one, so it was never taken.
    // Here the draft still equals what the daemon last reported, so it is taken.
    expect(shouldAdoptCurve(EDITED, BASE, BASE)).toBe(true);
  });
});

// --- The drag itself, through the pointer events the card listens for --------
//
// jsdom has no PointerEvent and no pointer capture; test-setup.ts supplies both
// so these can run. Without that, `fireEvent.pointerDown` silently does nothing
// and a test like this would pass for the wrong reason.

const palette = {
  primary: [120, 200, 255] as [number, number, number],
  secondary: [180, 120, 255] as [number, number, number],
  swatches: [] as Array<[number, number, number]>,
};

const asCurve = (cpu: CurvePoint[]): FanCurve => ({
  fan_count: 2,
  init_mode: 0,
  kb_type: 6,
  cpu,
  gpu1: BASE,
  gpu2: [P(0, 0), P(0, 0), P(0, 0), P(0, 0)],
});

/** CurveCard's geometry: W=320, H=150, PAD=18. */
const toPx = (temp: number, duty: number) => ({
  x: 18 + (temp / 100) * (320 - 36),
  y: 150 - 18 - (duty / 100) * (150 - 36),
});

function cpuText(): string {
  return screen.getByText(/^CPU:/).parentElement?.textContent ?? "";
}

function setup(curve: FanCurve) {
  const view = render(
    <CurveCard palette={palette} curve={curve} writable onApplied={() => {}} />,
  );
  // The card also contains two 24x24 icon SVGs, so select by the chart's
  // viewBox rather than by tag - `querySelector("svg")` gets an icon and the
  // pointer events go nowhere.
  const svg = document.querySelector('svg[viewBox="0 0 320 150"]')!;
  // jsdom performs no layout, so getBoundingClientRect is all zeros; pin it to
  // the viewBox so the pointer maths lands where the points are drawn.
  svg.getBoundingClientRect = () =>
    ({ left: 0, top: 0, width: 320, height: 150, right: 320, bottom: 150, x: 0, y: 0 }) as DOMRect;
  return { view, svg };
}

/** Drag the point at `from` (a temp/duty pair) to the `to` pair. */
function drag(svg: Element, from: [number, number], to: [number, number]) {
  const a = toPx(from[0], from[1]);
  const b = toPx(to[0], to[1]);
  fireEvent.pointerDown(svg, { clientX: a.x, clientY: a.y, pointerId: 1 });
  fireEvent.pointerMove(svg, { clientX: b.x, clientY: b.y, pointerId: 1 });
  fireEvent.pointerUp(svg, { clientX: b.x, clientY: b.y, pointerId: 1 });
}

describe("CurveCard dragging", () => {
  it("moves a point under the pointer", () => {
    const { svg } = setup(asCurve(BASE));
    drag(svg, [60, 36], [72, 80]);
    expect(cpuText()).toContain("(72°C,80%)");
  });

  it("keeps the edit when a poll re-renders with the same curve content", () => {
    // The regression: releasing a point set `dragging` back to null, which
    // re-ran the adopt effect and snapped the curve back to the EC's values.
    const { view, svg } = setup(asCurve(BASE));
    drag(svg, [60, 36], [72, 80]);
    const afterDrag = cpuText();
    expect(afterDrag).toContain("(72°C,80%)");

    // The 2-second poll: a fresh array with identical content.
    view.rerender(
      <CurveCard palette={palette} curve={asCurve(BASE.map((p) => ({ ...p })))} writable onApplied={() => {}} />,
    );

    expect(cpuText()).toBe(afterDrag);
  });

  it("still adopts a curve whose content really changed", () => {
    const { view } = setup(asCurve(BASE));
    expect(cpuText()).toContain("(60°C,36%)");
    view.rerender(
      <CurveCard palette={palette} curve={asCurve(EDITED)} writable onApplied={() => {}} />,
    );
    expect(cpuText()).toContain("(72°C,80%)");
  });

  it("keeps an unsaved edit when a different curve arrives", () => {
    const { view, svg } = setup(asCurve(BASE));
    drag(svg, [60, 36], [72, 80]);
    view.rerender(
      <CurveCard palette={palette} curve={asCurve([P(40, 30), P(50, 40), P(70, 60), P(90, 90)])} writable onApplied={() => {}} />,
    );
    // The user's work wins until they apply or reset it.
    expect(cpuText()).toContain("(72°C,80%)");
  });
});
