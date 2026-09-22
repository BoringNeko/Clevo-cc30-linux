import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, fireEvent, screen, waitFor } from "@testing-library/react";
import {
  CurveCard,
  FACTORY_CURVE,
  curvePath,
  dutyAtTemp,
  isEditablePoint,
  sameCurve,
  shouldAdoptCurve,
  tempRangeOf,
} from "./CurveCard";
import { setFanCurve } from "../api/daemon";
import type { CurvePoint, FanCurve } from "../api/daemon";

vi.mock("../api/daemon", async () => {
  const actual = await vi.importActual<typeof import("../api/daemon")>("../api/daemon");
  return { ...actual, setFanCurve: vi.fn().mockResolvedValue(undefined) };
});

const mockedSetFanCurve = vi.mocked(setFanCurve);

beforeEach(() => {
  mockedSetFanCurve.mockReset().mockResolvedValue(undefined);
});

const P = (temp: number, duty_pct: number): CurvePoint => ({ temp, duty_pct });

const BASE = [P(40, 25), P(60, 36), P(80, 53), P(100, 100)];
const EDITED = [P(40, 25), P(72, 80), P(80, 53), P(100, 100)];

/** A GPU1 curve that does not coincide with BASE, for unambiguous grabs. */
const GPU_OTHER = [P(45, 60), P(65, 70), P(85, 82), P(100, 100)];

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

const asCurve = (cpu: CurvePoint[], gpu1: CurvePoint[] = BASE): FanCurve => ({
  fan_count: 2,
  init_mode: 0,
  kb_type: 6,
  cpu,
  gpu1,
  gpu2: [P(0, 0), P(0, 0), P(0, 0), P(0, 0)],
});

/**
 * The axis spans the most recent `setup` produced.
 *
 * The card derives both axes from the curve it is given, and each test renders
 * exactly one card, so the helpers can read the ranges back from here instead of
 * making every call site thread them through.
 */
let currentTempRange: { lo: number; hi: number } = { lo: 0, hi: 100 };
const currentDutyRange = { lo: 0, hi: 100 };

/** The span the card derives for the temperature axis, mirroring `tempRangeOf`. */
function spanOf(values: number[]): { lo: number; hi: number } {
  const lo = Math.min(...values);
  return { lo, hi: Math.max(lo + 1, Math.max(...values)) };
}

/** Map a value in `range` to 0..100 across the plot's data area. */
function toPct(value: number, range: { lo: number; hi: number }): number {
  return ((value - range.lo) / (range.hi - range.lo)) * 100;
}

/**
 * The four points of a fan, recovered from where they are plotted.
 *
 * Only the two editable points have handles, and their positions are read back
 * directly. The first and last belong to the EC and have no marker, so they are
 * recovered from the curve: `curvePath` emits one `C` per gap and each segment
 * ends on the next point, so the path still contains all four.
 */
function pointsOf(fan: "CPU" | "GPU1"): string[] {
  const handles = Array.from(document.querySelectorAll<HTMLElement>('[data-testid="curve-handle"]'));
  const slice = fan === "CPU" ? handles.slice(0, 2) : handles.slice(2, 4);
  const span = 100 - 2 * PAD_PCT;
  const fromHandle = (el: HTMLElement) => {
    const style = getComputedStyle(el);
    return {
      x: ((parseFloat(style.left) - PAD_PCT) / span) * 100,
      y: ((100 - PAD_PCT - parseFloat(style.top)) / span) * 100,
    };
  };

  // The curve is the only place the fixed ends still appear.
  const d = document.querySelector(`[data-testid="curve-line-${fan.toLowerCase()}"]`)?.getAttribute("d") ?? "";
  const nums = d.match(/-?[\d.]+/g)?.map(Number) ?? [];
  // "M x y" then one "C c1x c1y c2x c2y x y" per gap (six numbers each). The
  // curve passes through the M and through each C's own endpoint, which is the
  // third pair of the group.
  const fromPath: Array<{ x: number; y: number }> = [];
  if (nums.length >= 8) {
    fromPath.push({ x: nums[0], y: nums[1] });
    for (let g = 2; g + 5 < nums.length; g += 6) {
      fromPath.push({ x: nums[g + 4], y: nums[g + 5] });
    }
  }

  const editable = slice.map(fromHandle);
  // `fromPath` holds path coordinates (viewBox units, including the inset);
  // `editable` holds percentages of the data area. Normalise both before
  // converting.
  const fromPathPct = fromPath.map((p) => ({
    x: ((p.x - PAD_PCT) / span) * 100,
    y: ((100 - PAD_PCT - p.y) / span) * 100,
  }));
  const pts = [
    fromPathPct[0] ?? editable[0],
    editable[0],
    editable[1],
    fromPathPct[fromPathPct.length - 1] ?? editable[1],
  ];
  const unit = (v: number, r: { lo: number; hi: number }) => Math.round(r.lo + (v / 100) * (r.hi - r.lo));
  return pts.map((p) => `${unit(p.x, currentTempRange)}°C${unit(p.y, currentDutyRange)}%`);
}

/** The first and last points of a fan's curve, read from its plotted path. */
function pathEnds(fan: "cpu" | "gpu1"): CurvePoint[] {
  const d = document.querySelector(`[data-testid="curve-line-${fan}"]`)?.getAttribute("d") ?? "";
  const nums = d.match(/-?[\d.]+/g)?.map(Number) ?? [];
  if (nums.length < 8) throw new Error(`no curve path for ${fan}`);
  const span = 100 - 2 * PAD_PCT;
  const at = (x: number, y: number) => ({
    temp: Math.round(currentTempRange.lo + (((x - PAD_PCT) / span) * 100 / 100) * (currentTempRange.hi - currentTempRange.lo)),
    // y is measured from the top, so the duty is its mirror.
    duty_pct: Math.round(
      currentDutyRange.lo +
        (((100 - PAD_PCT - y) / span)) * (currentDutyRange.hi - currentDutyRange.lo),
    ),
  });
  return [at(nums[0], nums[1]), at(nums[nums.length - 2], nums[nums.length - 1])];
}

/** A stable one-line summary of a fan's curve, for equality assertions. */
function summaryOf(fan: "CPU" | "GPU1"): string {
  return pointsOf(fan).join(" | ");
}

/**
 * The chart element's on-screen size.
 *
 * jsdom performs no layout, so `getBoundingClientRect` is all zeros and must be
 * stubbed. The size deliberately differs from a square: the coordinate mapping
 * maps the whole rectangle onto the plot, so a non-square stub catches an
 * implementation that assumes the two axes scale alike.
 */
const DEFAULT_RECT = { left: 0, top: 0, width: 700, height: 300 };

/** Padding, as a percentage of the chart box, that the plot leaves unused. */
const PAD_PCT = 4;

function setup(curve: FanCurve, rect: { left: number; top: number; width: number; height: number } = DEFAULT_RECT) {
  const view = render(
    <CurveCard palette={palette} curve={curve} writable onApplied={() => {}} />,
  );
  // The plot is the positioned box that holds the chart SVG; the pointer maths
  // reads its rectangle, not the SVG's (which stretches to fill it). The card
  // also contains icon SVGs, so select the chart by its viewBox.
  const svg = document.querySelector('svg[viewBox="0 0 100 100"]')!;
  const chart = svg.parentElement as HTMLElement;
  chart.getBoundingClientRect = () =>
    ({
      ...rect,
      right: rect.left + rect.width,
      bottom: rect.top + rect.height,
      x: rect.left,
      y: rect.top,
    }) as DOMRect;
  currentTempRange = spanOf([...curve.cpu, ...curve.gpu1].map((p) => p.temp));
  return { view, svg, chart, rect, range: currentTempRange };
}

/**
 * Where a (temp, duty) point appears on screen, for a given chart rectangle.
 *
 * The plot places temp/duty into an inner box inset by `PAD_PCT` on each side,
 * then stretches that box across the element. Mirroring it here is what makes
 * the simulated pointer land where the browser would really put the handle; a
 * test that computed positions any other way would agree with a buggy mapping.
 */
function pointOnScreen(
  temp: number,
  duty: number,
  rect: { left: number; top: number; width: number; height: number },
  tempRange: { lo: number; hi: number } = currentTempRange,
  dutyRange: { lo: number; hi: number } = currentDutyRange,
) {
  const span = 100 - 2 * PAD_PCT;
  const xPct = PAD_PCT + (toPct(temp, tempRange) / 100) * span;
  const yPct = PAD_PCT + ((100 - toPct(duty, dutyRange)) / 100) * span;
  return {
    x: rect.left + (xPct / 100) * rect.width,
    y: rect.top + (yPct / 100) * rect.height,
  };
}

/** Drag the point at `from` (a temp/duty pair) to the `to` pair. */
function drag(
  svg: Element,
  from: [number, number],
  to: [number, number],
  rect: { left: number; top: number; width: number; height: number } = DEFAULT_RECT,
  tempRange: { lo: number; hi: number } = currentTempRange,
  dutyRange: { lo: number; hi: number } = currentDutyRange,
) {
  const a = pointOnScreen(from[0], from[1], rect, tempRange, dutyRange);
  const b = pointOnScreen(to[0], to[1], rect, tempRange, dutyRange);
  fireEvent.pointerDown(svg, { clientX: a.x, clientY: a.y, pointerId: 1 });
  fireEvent.pointerMove(svg, { clientX: b.x, clientY: b.y, pointerId: 1 });
  fireEvent.pointerUp(svg, { clientX: b.x, clientY: b.y, pointerId: 1 });
}

describe("CurveCard dragging", () => {
  it("moves a CPU point under the pointer", () => {
    // Distinct curves so the grab is unambiguous; see the tie-break test below.
    const { svg } = setup(asCurve(BASE, GPU_OTHER));
    drag(svg, [60, 36], [72, 80]);
    expect(pointsOf("CPU")).toContain("72°C80%");
    // The other curve is untouched.
    expect(pointsOf("GPU1")).toContain("65°C70%");
  });

  it("moves a GPU1 point too, independently of the CPU", () => {
    // Both lines are editable. Give them distinct curves so the grab is
    // unambiguous - coincident points would make the hit test (and a real user)
    // unable to tell which line is being dragged.
    const gpu = [P(45, 60), P(65, 70), P(85, 82), P(100, 100)];
    const { svg } = setup(asCurve(BASE, gpu));

    drag(svg, [85, 82], [70, 20]);

    expect(pointsOf("GPU1")).toContain("70°C20%");
    // The CPU curve is untouched.
    expect(pointsOf("CPU")).toContain("80°C53%");
    expect(pointsOf("CPU")).toContain("60°C36%");
  });

  it("keeps the edit when a poll re-renders with the same curve content", () => {
    // The regression: releasing a point used to re-run the adopt effect and
    // snap the curve back to the EC's values.
    const { view, svg } = setup(asCurve(BASE, GPU_OTHER));
    drag(svg, [60, 36], [72, 80]);
    const afterDrag = summaryOf("CPU");
    expect(pointsOf("CPU")).toContain("72°C80%");

    // The 2-second poll: a fresh array with identical content.
    view.rerender(
      <CurveCard
        palette={palette}
        curve={asCurve(
          BASE.map((p) => ({ ...p })),
          GPU_OTHER.map((p) => ({ ...p })),
        )}
        writable
        onApplied={() => {}}
      />,
    );

    expect(summaryOf("CPU")).toBe(afterDrag);
  });

  it("still adopts a curve whose content really changed", () => {
    const { view } = setup(asCurve(BASE));
    expect(pointsOf("CPU")).toContain("60°C36%");
    view.rerender(
      <CurveCard palette={palette} curve={asCurve(EDITED)} writable onApplied={() => {}} />,
    );
    expect(pointsOf("CPU")).toContain("72°C80%");
  });

  it("keeps an unsaved edit when a different curve arrives", () => {
    const { view, svg } = setup(asCurve(BASE, [P(45, 60), P(65, 70), P(85, 82), P(100, 100)]));
    drag(svg, [60, 36], [72, 80]);
    view.rerender(
      <CurveCard
        palette={palette}
        curve={asCurve([P(40, 30), P(50, 40), P(70, 60), P(90, 90)])}
        writable
        onApplied={() => {}}
      />,
    );
    // The user's work wins until they apply or reset it.
    expect(pointsOf("CPU")).toContain("72°C80%");
  });

  it("gives a coincident point to the upper line (GPU1)", () => {
    // Both curves identical: the hit test must pick the one drawn on top, and
    // the drag label tells the user which they grabbed.
    const { svg } = setup(asCurve(BASE, BASE));
    drag(svg, [60, 36], [72, 80]);
    expect(pointsOf("GPU1")).toContain("72°C80%");
    expect(pointsOf("CPU")).toContain("60°C36%");
  });

  it("moves the third point of each line", () => {
    // The reported bug: only the second point could be grabbed, because the
    // fixed first/last points were also candidates and stole near misses.
    const { svg } = setup(asCurve(BASE, GPU_OTHER));

    drag(svg, [80, 53], [70, 25]);
    expect(pointsOf("CPU")).toContain("70°C25%");

    drag(svg, [85, 82], [72, 30]);
    expect(pointsOf("GPU1")).toContain("72°C30%");
  });

  it("does not move the firmware-owned first and last points", () => {
    // Command 14 carries only the middle two points, so the ends belong to the
    // EC. There is no handle on them, so a drag aimed at one either does
    // nothing or grabs a nearby editable point - either way the ends hold.
    const { svg } = setup(asCurve(BASE, GPU_OTHER));

    drag(svg, [40, 25], [15, 90]); // at the first point
    drag(svg, [100, 100], [40, 10]); // at the last point

    // The fixed ends are still where they started, on both curves.
    expect(pathEnds("cpu")).toEqual([P(40, 25), P(100, 100)]);
    expect(pathEnds("gpu1")).toEqual([P(45, 60), P(100, 100)]);
  });
});

describe("isEditablePoint", () => {
  it("only the middle two points of a four-point curve can move", () => {
    expect(isEditablePoint(0)).toBe(false);
    expect(isEditablePoint(1)).toBe(true);
    expect(isEditablePoint(2)).toBe(true);
    expect(isEditablePoint(3)).toBe(false);
  });
});

describe("CurveCard coordinate mapping", () => {
  // The chart element is rarely square, and the mapping stretches the plot
  // across the whole element. Testing only at one size hides an implementation
  // that assumes a fixed aspect ratio.
  const SIZES = [
    { label: "wide", rect: { left: 0, top: 0, width: 700, height: 300 } },
    { label: "tall", rect: { left: 0, top: 0, width: 300, height: 400 } },
    { label: "square", rect: { left: 0, top: 0, width: 400, height: 400 } },
    { label: "offset", rect: { left: 40, top: 25, width: 640, height: 260 } },
  ];

  for (const { label, rect } of SIZES) {
    it(`grabs each point when the chart is ${label}`, () => {
      const { svg } = setup(asCurve(BASE, GPU_OTHER), rect);

      // Move each editable point in turn; every one must be picked up.
      drag(svg, [60, 36], [58, 30], rect);
      expect(pointsOf("CPU")).toContain("58°C30%");

      drag(svg, [80, 53], [78, 60], rect);
      expect(pointsOf("CPU")).toContain("78°C60%");

      drag(svg, [65, 70], [63, 66], rect);
      expect(pointsOf("GPU1")).toContain("63°C66%");

      drag(svg, [85, 82], [83, 78], rect);
      expect(pointsOf("GPU1")).toContain("83°C78%");
    });
  }

  it("declares the stretch that the pointer maths assumes", () => {
    // Without this the drawing is letterboxed inside the element and every
    // coordinate is off by the size of the margin.
    const { svg } = setup(asCurve(BASE));
    expect(svg.getAttribute("preserveAspectRatio")).toBe("none");
  });
});

describe("tempRangeOf", () => {
  it("spans the outermost temperatures of both fans", () => {
    expect(tempRangeOf([BASE, GPU_OTHER])).toEqual({ lo: 40, hi: 100 });
  });

  it("takes the union, not just one fan", () => {
    // CPU starts at 40, GPU1 ends at 95: the axis must cover both.
    const cpu = [P(40, 20), P(50, 30), P(60, 40), P(70, 50)];
    const gpu = [P(45, 20), P(60, 40), P(80, 60), P(95, 90)];
    expect(tempRangeOf([cpu, gpu])).toEqual({ lo: 40, hi: 95 });
  });

  it("ignores an empty channel", () => {
    expect(tempRangeOf([BASE, []])).toEqual({ lo: 40, hi: 100 });
  });

  it("falls back to a full scale when there is nothing to plot", () => {
    expect(tempRangeOf([[], []])).toEqual({ lo: 0, hi: 100 });
  });

  it("keeps a non-zero span rather than dividing by zero", () => {
    const flat = [P(50, 10), P(50, 20), P(50, 30), P(50, 40)];
    expect(tempRangeOf([flat])).toEqual({ lo: 50, hi: 51 });
  });
});

describe("curvePath", () => {
  it("starts at the first point's plotted position", () => {
    const d = curvePath([P(40, 25), P(60, 36), P(80, 53), P(100, 100)], 4);
    // temp 40 -> 4 + 0.4*92 = 40.8 ; duty 25 -> 4 + 0.75*92 = 73
    const [x, y] = d.slice(2).split(" C")[0].trim().split(/\s+/).map(Number);
    expect(x).toBeCloseTo(40.8, 6);
    expect(y).toBeCloseTo(73, 6);
  });

  it("uses a cubic segment per gap, so the line is smooth", () => {
    const d = curvePath([P(40, 25), P(60, 36), P(80, 53), P(100, 100)], 4);
    // Three gaps, three curves, and no straight `L` left.
    expect(d.match(/C /g)).toHaveLength(3);
    expect(d).not.toContain("L");
  });

  it("is empty for an empty curve", () => {
    expect(curvePath([], 4)).toBe("");
  });
});

describe("dutyAtTemp", () => {
  it("reads a point's own duty", () => {
    expect(dutyAtTemp(60, BASE)).toBe(36);
  });

  it("interpolates between points", () => {
    // Halfway from (60,36) to (80,53).
    expect(dutyAtTemp(70, BASE)).toBe(45);
  });

  it("clamps beyond the first and last point", () => {
    expect(dutyAtTemp(20, BASE)).toBe(25);
    expect(dutyAtTemp(110, BASE)).toBe(100);
  });
});

describe("CurveCard applying", () => {
  it("has an apply button that is disabled until something is edited", () => {
    const { svg } = setup(asCurve(BASE, GPU_OTHER));
    expect(screen.getByRole("button", { name: /保存配置/ })).toBeDisabled();

    // After an edit it becomes usable.
    drag(svg, [60, 36], [72, 80]);
    expect(screen.getByRole("button", { name: /保存配置/ })).toBeEnabled();
  });

  it("sends both edited curves to the daemon", async () => {
    const { svg } = setup(asCurve(BASE, GPU_OTHER));
    // Move a CPU point and a GPU1 point.
    drag(svg, [60, 36], [72, 80]);
    drag(svg, [85, 82], [70, 20]);

    fireEvent.click(screen.getByRole("button", { name: /保存配置/ }));

    await waitFor(() => expect(mockedSetFanCurve).toHaveBeenCalledTimes(1));
    const sent = mockedSetFanCurve.mock.calls[0][0] as FanCurve;
    expect(sent.cpu.find((p) => p.temp === 60)).toBeUndefined();
    expect(sent.cpu).toContainEqual(P(72, 80));
    expect(sent.gpu1).toContainEqual(P(70, 20));
  });

  it("reports which fans were written", async () => {
    const { svg } = setup(asCurve(BASE, GPU_OTHER));
    drag(svg, [60, 36], [72, 80]);
    fireEvent.click(screen.getByRole("button", { name: /保存配置/ }));
    expect(await screen.findByText(/CPU 曲线/)).toBeTruthy();
  });
});

describe("CurveCard text selection", () => {
  /**
   * The reported bug: pressing on a point and sweeping the pointer across the
   * axis labels started a native text selection, so the gesture became "drag the
   * selection" - the point stopped following and the chart lost the pointer.
   */
  it("cancels the default action when a drag starts", () => {
    const { svg } = setup(asCurve(BASE, GPU_OTHER));
    const a = pointOnScreen(60, 36, DEFAULT_RECT);
    // `dispatchEvent` returns false when a listener called preventDefault.
    const allowed = fireEvent.pointerDown(svg, { clientX: a.x, clientY: a.y, pointerId: 1 });
    expect(allowed).toBe(false);
  });

  it("does not cancel the default action when nothing was grabbed", () => {
    // Only the chart's own presses are ours to consume; a press in empty space
    // must stay a normal click.
    const { svg } = setup(asCurve(BASE, GPU_OTHER));
    // The readout row is 77px tall and the chart starts below it, so this point
    // is outside every handle's grab radius.
    const far = pointOnScreen(95, 5, DEFAULT_RECT);
    const allowed = fireEvent.pointerDown(svg, { clientX: far.x, clientY: far.y, pointerId: 1 });
    expect(allowed).toBe(true);
  });

  it("makes the card unselectable, so a sweeping drag cannot select the labels", () => {
    setup(asCurve(BASE, GPU_OTHER));
    const card = document.querySelector(".MuiCard-root") as HTMLElement;
    expect(getComputedStyle(card).userSelect).toBe("none");
  });
});

describe("CurveCard handles", () => {
  /**
   * The handle boxes, in DOM order: CPU's four then GPU1's four.
   *
   * The positions live in Emotion's stylesheet rather than in inline styles,
   * so `getComputedStyle` is what actually reports where a box lands.
   */
  function handleBoxes(): HTMLElement[] {
    return Array.from(document.querySelectorAll<HTMLElement>('[data-testid="curve-handle"]'));
  }

  it("renders handles for the editable points only", () => {
    // Two per fan: the first and last belong to the EC and get no marker, so a
    // drag can never be started on a point that is not written back.
    setup(asCurve(BASE, GPU_OTHER));
    expect(handleBoxes()).toHaveLength(4);
  });

  it("centres each handle on its point instead of hanging below it", () => {
    // The bug: `top` puts the box's top edge at the point, so a
    // `translate(-50%, 50%)` dropped every handle a full handle-height too low
    // - a shift the drag tests could not see, because they aim at the data
    // coordinate rather than at the rendered box.
    setup(asCurve(BASE, GPU_OTHER));
    for (const el of handleBoxes()) {
      expect(getComputedStyle(el).transform).toContain("translate(-50%, -50%)");
    }
  });

  it("places a handle at the percentage its point maps to", () => {
    setup(asCurve(BASE, GPU_OTHER));
    // Only the editable points have handles, so [0] is CPU point 2 (60°C, 36%)
    // and [1] is CPU point 3 (80°C, 53%).
    // Temperature axis spans 40..100, so point 2 is (60-40)/60 = 33.3% across:
    // x = 4 + 0.3333*92 = 34.67. Duty is the fixed 0..100 %, so 36% is 64%
    // down: y = 4 + 0.64*92 = 62.88.
    const second = getComputedStyle(handleBoxes()[0]);
    expect(parseFloat(second.left)).toBeCloseTo(34.666666, 5);
    expect(parseFloat(second.top)).toBeCloseTo(62.88, 5);
    // Point 3: (80-40)/60 = 66.7% across, 53% duty is 47% down.
    const third = getComputedStyle(handleBoxes()[1]);
    expect(parseFloat(third.left)).toBeCloseTo(4 + 0.666666 * 92, 3);
    expect(parseFloat(third.top)).toBeCloseTo(4 + 0.47 * 92, 4);
  });

  it("puts the curve's first and last points on the axis ends", () => {
    // The temperature axis is derived from the curve, so its ends must coincide
    // with the outermost points. The reported bug was the curve starting a
    // third of the way in, because the axis was a fixed 0..100 °C while the
    // curve occupies 40..100.
    setup(asCurve(BASE, GPU_OTHER));
    const d = Array.from(document.querySelectorAll("path"))
      .map((el) => el.getAttribute("d") ?? "")
      .find((v) => v.includes("C")) ?? "";
    const nums = d.match(/-?[\d.]+/g)?.map(Number) ?? [];
    // "M x y C …": the first coordinate is the curve's first point (40 °C), the
    // second-to-last is the last point's x (100 °C). Both should sit on the ends
    // of the data area, which is inset by PAD_PCT.
    expect(nums[0]).toBeCloseTo(PAD_PCT, 6);
    expect(nums[nums.length - 2]).toBeCloseTo(100 - PAD_PCT, 6);
  });

  it("gives every handle a solid, movable look", () => {
    setup(asCurve(BASE, GPU_OTHER));
    // Four handles total, and every one is a solid marker in its fan's colour:
    // there is no hollow "not yours" state any more, because the fixed points
    // get no handle at all.
    const colors = handleBoxes().map((el) => getComputedStyle(el).backgroundColor);
    expect(colors).toEqual([
      "rgb(120, 200, 255)",
      "rgb(120, 200, 255)",
      "rgb(180, 120, 255)",
      "rgb(180, 120, 255)",
    ]);
  });
});

describe("CurveCard readout", () => {
  /** Text of the readout block above the chart, whitespace-normalised. */
  function readoutText(): string {
    return (screen.getByTestId("curve-readout").textContent ?? "").replace(/\s+/g, " ");
  }

  it("shows the live temperature and the duty the curve gives there", () => {
    render(
      <CurveCard
        palette={palette}
        curve={asCurve(BASE, GPU_OTHER)}
        writable
        onApplied={() => {}}
        temps={{ cpu: 75, gpu1: 75 }}
      />,
    );
    // 75 C sits three quarters of the way from (60,36) to (80,53) -> 49% for
    // the CPU curve; the GPU1 curve at 75 C is halfway from (65,70) to
    // (85,82) -> 76%.
    expect(readoutText()).toContain("75°C");
    expect(readoutText()).toContain("49%");
    expect(readoutText()).toContain("76%");
  });

  it("shows a dash when no temperature is known", () => {
    render(<CurveCard palette={palette} curve={asCurve(BASE, GPU_OTHER)} writable onApplied={() => {}} />);
    expect(readoutText()).toContain("—");
  });

  it("shows the dragged point's own value while it is held", () => {
    const { svg } = setup(asCurve(BASE, GPU_OTHER));
    // Press and move but do not release, so the drag is still in progress.
    const from = pointOnScreen(60, 36, DEFAULT_RECT);
    fireEvent.pointerDown(svg, { clientX: from.x, clientY: from.y, pointerId: 1 });
    const to = pointOnScreen(72, 80, DEFAULT_RECT);
    fireEvent.pointerMove(svg, { clientX: to.x, clientY: to.y, pointerId: 1 });

    // The readout switches to that point's (temp, duty) rather than the live one.
    expect(readoutText()).toContain("72°C");
    expect(readoutText()).toContain("80%");
  });
});

describe("CurveCard restore buttons", () => {
  it("还原配置 discards the edit and does not write", () => {
    const { svg } = setup(asCurve(BASE, GPU_OTHER));
    drag(svg, [60, 36], [72, 80]);
    expect(pointsOf("CPU")).toContain("72°C80%");

    fireEvent.click(screen.getByRole("button", { name: /还原配置/ }));

    expect(pointsOf("CPU")).toContain("60°C36%");
    expect(mockedSetFanCurve).not.toHaveBeenCalled();
  });

  it("还原默认 loads the factory curve without writing", () => {
    const { svg } = setup(asCurve(BASE, GPU_OTHER));
    drag(svg, [60, 36], [15, 90]);

    fireEvent.click(screen.getByRole("button", { name: /还原默认/ }));
    // The factory CPU curve, straight from hardware-notes.
    expect(pointsOf("CPU")).toEqual([
      "40°C25%",
      "60°C36%",
      "80°C53%",
      "100°C100%",
    ]);
    // Nothing reaches the EC until 保存配置.
    expect(mockedSetFanCurve).not.toHaveBeenCalled();
  });

  it("saves the factory curve once 保存配置 is pressed", async () => {
    const { svg } = setup(asCurve([P(40, 10), P(50, 10), P(60, 10), P(100, 10)]));
    drag(svg, [50, 10], [55, 90]);

    fireEvent.click(screen.getByRole("button", { name: /还原默认/ }));
    fireEvent.click(screen.getByRole("button", { name: /保存配置/ }));

    await waitFor(() => expect(mockedSetFanCurve).toHaveBeenCalledTimes(1));
    const sent = mockedSetFanCurve.mock.calls[0][0] as FanCurve;
    expect(sent.cpu).toEqual(FACTORY_CURVE.cpu);
    expect(sent.gpu1).toEqual(FACTORY_CURVE.gpu1);
  });

  it("offers 还原默认 even when nothing has been edited yet", () => {
    setup(asCurve(BASE, GPU_OTHER));
    // Restoring the default is meaningful before any edit (the EC may already
    // hold something else), so it must not be gated on a dirty draft.
    expect(screen.getByRole("button", { name: /还原默认/ })).toBeEnabled();
    expect(screen.getByRole("button", { name: /还原配置/ })).toBeDisabled();
  });
});
