import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, fireEvent, screen, waitFor } from "@testing-library/react";
import { CurveCard, isEditablePoint, sameCurve, shouldAdoptCurve } from "./CurveCard";
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
 * The four points shown for a fan, read from its table row.
 *
 * Returns e.g. ["40°C 25%", "60°C36%", ...] by joining each cell's two spans,
 * which is what the table renders per point.
 */
function pointsOf(fan: "CPU" | "GPU1"): string[] {
  const row = screen
    .getAllByRole("row")
    .find((r) => r.querySelector("td")?.textContent === fan);
  if (!row) throw new Error(`no table row for ${fan}`);
  const cells = Array.from(row.querySelectorAll("td")).slice(1);
  return cells.map((c) => (c.textContent ?? "").replace(/\s+/g, " ").trim());
}

/** A stable one-line summary of a fan's curve, for equality assertions. */
function summaryOf(fan: "CPU" | "GPU1"): string {
  return pointsOf(fan).join(" | ");
}

/**
 * The element's on-screen size.
 *
 * jsdom performs no layout, so `getBoundingClientRect` is all zeros and must be
 * stubbed. The default deliberately differs from the 320:150 viewBox - a real
 * card is about 2:1 while the viewBox is 2.13:1 - because a stubbed rect that
 * exactly matches the viewBox hides any error in the coordinate mapping.
 */
const DEFAULT_RECT = { left: 0, top: 0, width: 700, height: 300 };

function setup(curve: FanCurve, rect: { left: number; top: number; width: number; height: number } = DEFAULT_RECT) {
  const view = render(
    <CurveCard palette={palette} curve={curve} writable onApplied={() => {}} />,
  );
  // The card also contains two 24x24 icon SVGs, so select by the chart's
  // viewBox rather than by tag - `querySelector("svg")` gets an icon and the
  // pointer events go nowhere.
  const svg = document.querySelector('svg[viewBox="0 0 320 150"]')!;
  svg.getBoundingClientRect = () =>
    ({
      ...rect,
      right: rect.left + rect.width,
      bottom: rect.top + rect.height,
      x: rect.left,
      y: rect.top,
    }) as DOMRect;
  return { view, svg, rect };
}

/**
 * Where a (temp, duty) point appears on screen, for a given element rectangle.
 *
 * This mirrors how the browser maps the viewBox onto the element, including
 * `preserveAspectRatio`. When the ratios differ, the default ("xMidYMid meet")
 * scales the drawing to fit and *centres* it, leaving margins; "none" stretches
 * it to fill. Getting this wrong in the test is what let the mapping bug hide:
 * computing screen positions as if the drawing always filled the element made
 * the component's own (incorrect) assumption look right.
 */
function pointOnScreen(
  temp: number,
  duty: number,
  rect: { left: number; top: number; width: number; height: number },
  preserveAspectRatio: string = "none",
) {
  const vx = 18 + (temp / 100) * (320 - 36);
  const vy = 150 - 18 - (duty / 100) * (150 - 36);

  let scaleX = rect.width / 320;
  let scaleY = rect.height / 150;
  let offsetX = rect.left;
  let offsetY = rect.top;

  if (preserveAspectRatio !== "none") {
    const scale = Math.min(scaleX, scaleY);
    scaleX = scale;
    scaleY = scale;
    offsetX = rect.left + (rect.width - 320 * scale) / 2;
    offsetY = rect.top + (rect.height - 150 * scale) / 2;
  }

  return { x: offsetX + vx * scaleX, y: offsetY + vy * scaleY };
}

/** Drag the point at `from` (a temp/duty pair) to the `to` pair. */
function drag(
  svg: Element,
  from: [number, number],
  to: [number, number],
  rect: { left: number; top: number; width: number; height: number } = DEFAULT_RECT,
) {
  // Read the element's own declaration so the simulated pointer lands where the
  // browser would really put the point.
  const par = svg.getAttribute("preserveAspectRatio") ?? "";
  const a = pointOnScreen(from[0], from[1], rect, par);
  const b = pointOnScreen(to[0], to[1], rect, par);
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
  // The element rarely has the viewBox's 320:150 ratio. When it does not, the
  // browser either stretches the drawing or letterboxes it; the pointer maths
  // assumes the former. Testing only at 320x150 stubs the rectangle to match
  // the viewBox, which hides the discrepancy entirely.
  const SIZES = [
    { label: "wide", rect: { left: 0, top: 0, width: 700, height: 300 } },
    { label: "tall", rect: { left: 0, top: 0, width: 300, height: 400 } },
    { label: "exact", rect: { left: 0, top: 0, width: 320, height: 150 } },
    { label: "offset", rect: { left: 40, top: 25, width: 640, height: 260 } },
  ];

  for (const { label, rect } of SIZES) {
    it(`grabs each point when the element is ${label}`, () => {
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

describe("CurveCard applying", () => {
  it("has an apply button that is disabled until something is edited", () => {
    const { svg } = setup(asCurve(BASE, GPU_OTHER));
    expect(screen.getByRole("button", { name: /应用曲线/ })).toBeDisabled();

    // After an edit it becomes usable.
    drag(svg, [60, 36], [72, 80]);
    expect(screen.getByRole("button", { name: /应用曲线/ })).toBeEnabled();
  });

  it("sends both edited curves to the daemon", async () => {
    const { svg } = setup(asCurve(BASE, GPU_OTHER));
    // Move a CPU point and a GPU1 point.
    drag(svg, [60, 36], [72, 80]);
    drag(svg, [85, 82], [70, 20]);

    fireEvent.click(screen.getByRole("button", { name: /应用曲线/ }));

    await waitFor(() => expect(mockedSetFanCurve).toHaveBeenCalledTimes(1));
    const sent = mockedSetFanCurve.mock.calls[0][0] as FanCurve;
    expect(sent.cpu.find((p) => p.temp === 60)).toBeUndefined();
    expect(sent.cpu).toContainEqual(P(72, 80));
    expect(sent.gpu1).toContainEqual(P(70, 20));
  });

  it("reports which fans were written", async () => {
    const { svg } = setup(asCurve(BASE, GPU_OTHER));
    drag(svg, [60, 36], [72, 80]);
    fireEvent.click(screen.getByRole("button", { name: /应用曲线/ }));
    expect(await screen.findByText(/CPU 曲线/)).toBeTruthy();
  });
});
