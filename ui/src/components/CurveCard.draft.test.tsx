import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, fireEvent, screen, waitFor } from "@testing-library/react";
import { CurveCard, FACTORY_CURVE, curvePath, dutyAtTemp, isEditablePoint, sameCurve, shouldAdoptCurve } from "./CurveCard";
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
 * The four points of a fan, read back from where its handles are drawn.
 *
 * The card no longer lists the values as text, so the geometry is the source of
 * truth: each handle is centred on its point, and the inverse of that mapping
 * recovers the (temp, duty) the curve holds.
 */
function pointsOf(fan: "CPU" | "GPU1"): string[] {
  const handles = Array.from(document.querySelectorAll<HTMLElement>('[data-testid="curve-handle"]'));
  const slice = fan === "CPU" ? handles.slice(0, 4) : handles.slice(4, 8);
  return slice.map((el) => {
    const style = getComputedStyle(el);
    const xPct = parseFloat(style.left);
    const yPct = parseFloat(style.top);
    const span = 100 - 2 * PAD_PCT;
    const temp = Math.round(((xPct - PAD_PCT) / span) * 100);
    const duty = Math.round(((100 - PAD_PCT - yPct) / span) * 100);
    return `${temp}°C${duty}%`;
  });
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
  return { view, svg, chart, rect };
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
) {
  const span = 100 - 2 * PAD_PCT;
  const xPct = PAD_PCT + (temp / 100) * span;
  const yPct = PAD_PCT + ((100 - duty) / 100) * span;
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
) {
  const a = pointOnScreen(from[0], from[1], rect);
  const b = pointOnScreen(to[0], to[1], rect);
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
    // Command 14 carries only the middle two points, so dragging the ends must
    // do nothing - they belong to the EC.
    const { svg } = setup(asCurve(BASE, GPU_OTHER));

    drag(svg, [40, 25], [15, 90]); // first point
    drag(svg, [100, 100], [60, 10]); // last point

    expect(pointsOf("CPU")).toContain("40°C25%");
    expect(pointsOf("CPU")).toContain("100°C100%");
    expect(pointsOf("GPU1")).toContain("45°C60%");
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

  it("renders one handle per curve point, both channels", () => {
    setup(asCurve(BASE, GPU_OTHER));
    expect(handleBoxes()).toHaveLength(8);
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
    // BASE point 2 is (60°C, 36%): x = 4 + 0.6*92 = 59.2, y = 4 + 0.64*92 = 62.88.
    const second = getComputedStyle(handleBoxes()[1]);
    expect(parseFloat(second.left)).toBeCloseTo(59.2, 6);
    expect(parseFloat(second.top)).toBeCloseTo(62.88, 6);
  });

  it("marks the firmware-owned ends as inert and the middle as editable", () => {
    setup(asCurve(BASE, GPU_OTHER));
    const cpu = handleBoxes().slice(0, 4).map((el) => getComputedStyle(el));
    // The ends stay hollow and dimmed; the middle points are solid.
    for (const style of [cpu[0], cpu[3]]) {
      expect(style.backgroundColor).toBe("rgba(0, 0, 0, 0)");
      expect(parseFloat(style.opacity)).toBeLessThan(1);
    }
    for (const style of [cpu[1], cpu[2]]) {
      expect(style.backgroundColor).toBe("rgb(120, 200, 255)");
      expect(parseFloat(style.opacity)).toBe(1);
    }
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
