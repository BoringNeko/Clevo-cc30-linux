import { describe, expect, it } from "vitest";
import { movePoint } from "./CurveCard";

const base = [
  { temp: 40, duty_pct: 25 },
  { temp: 60, duty_pct: 36 },
  { temp: 80, duty_pct: 53 },
  { temp: 100, duty_pct: 100 },
];

describe("movePoint", () => {
  it("updates the dragged point", () => {
    const next = movePoint(base, 1, 55, 40);
    expect(next[1]).toEqual({ temp: 55, duty_pct: 40 });
  });

  it("does not mutate the input", () => {
    const copy = base.map((p) => ({ ...p }));
    movePoint(base, 1, 55, 40);
    expect(base).toEqual(copy);
  });

  it("clamps duty into 0..100", () => {
    expect(movePoint(base, 1, 55, -20)[1].duty_pct).toBe(0);
    expect(movePoint(base, 1, 55, 480)[1].duty_pct).toBe(100);
  });

  it("keeps temperatures strictly increasing against the neighbourhood", () => {
    // Push point 2 hard against its neighbours in both directions.
    expect(movePoint(base, 1, 0, 40)[1].temp).toBe(42); // >= T1 + gap
    expect(movePoint(base, 1, 999, 40)[1].temp).toBe(78); // <= T3 - gap
  });

  it("clamps the endpoints to the axis range", () => {
    expect(movePoint(base, 0, -50, 10)[0].temp).toBe(0);
    expect(movePoint(base, 3, 500, 10)[3].temp).toBe(100);
  });

  it("keeps the whole curve monotonic through a worst-case drag", () => {
    let points = base;
    // Drag every point across the entire range; the curve must stay ordered.
    for (let i = 0; i < points.length; i++) {
      points = movePoint(points, i, 999, 50);
      points = movePoint(points, i, -999, 50);
      for (let j = 1; j < points.length; j++) {
        expect(points[j].temp).toBeGreaterThan(points[j - 1].temp);
      }
    }
  });
});
