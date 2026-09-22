import { describe, expect, it, vi, beforeEach } from "vitest";
import { useState } from "react";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { PerformanceCard, isFullWidthMode } from "./PerformanceCard";
import { CurveCard } from "./CurveCard";
import { CurveHintCard } from "./CurveHintCard";
import {
  CUSTOMIZE_FAN_MODE,
  isCustomizeMode,
  type FanSnapshot,
  type FanCurve,
} from "../api/daemon";

const setFanMode = vi.fn();
const setPerfMode = vi.fn();
const setFanCurve = vi.fn();

vi.mock("../api/daemon", async () => {
  // Keep the real constants/helpers (the mode table and the predicate) and stub
  // only the D-Bus writes.
  const actual = await vi.importActual<typeof import("../api/daemon")>("../api/daemon");
  return {
    ...actual,
    setFanMode: (mode: string) => setFanMode(mode),
    setPerfMode: (mode: string) => setPerfMode(mode),
    setFanCurve: (curve: FanCurve) => setFanCurve(curve),
  };
});

const palette = {
  primary: [120, 200, 255] as [number, number, number],
  secondary: [180, 120, 255] as [number, number, number],
  swatches: [] as Array<[number, number, number]>,
};

const CURVE: FanCurve = {
  fan_count: 2,
  init_mode: 0,
  kb_type: 6,
  cpu: [
    { temp: 40, duty_pct: 25 },
    { temp: 60, duty_pct: 36 },
    { temp: 80, duty_pct: 53 },
    { temp: 100, duty_pct: 100 },
  ],
  gpu1: [
    { temp: 40, duty_pct: 25 },
    { temp: 60, duty_pct: 36 },
    { temp: 80, duty_pct: 53 },
    { temp: 100, duty_pct: 100 },
  ],
  gpu2: [
    { temp: 0, duty_pct: 0 },
    { temp: 0, duty_pct: 0 },
    { temp: 0, duty_pct: 0 },
    { temp: 0, duty_pct: 0 },
  ],
};

function snapshot(fanMode: number): FanSnapshot {
  const reading = { rpm: 1200, temp_c: 45, available: true };
  return {
    cpu: reading,
    gpu1: reading,
    gpu2: { rpm: 0, temp_c: null, available: false },
    freshness: "fresh",
    fan_count: 2,
    fan_mode: fanMode,
    perf_mode: 2,
    writable: true,
    curve_writable: true,
  };
}

/**
 * The dashboard's wiring, reduced to what matters for the gate: the mode card
 * drives `fanMode`, and the curve slot renders either the editor or the hint
 * exactly as App.tsx does.
 *
 * The harness mirrors what App does after a write: the refresh re-reads the
 * daemon, and here that is modelled by applying the same mode the card sent.
 */
function Dashboard({ initialMode }: { initialMode: number }) {
  const [fanMode, applyMode] = useState(initialMode);
  // What the "daemon" reports back after the last write: the mode the card
  // sent. Reading the mock's own record avoids threading state through
  // PerformanceCard, whose onRefresh takes no arguments (it just says "re-read").
  const reported = (): number => {
    const calls = setFanMode.mock.calls;
    const sent = calls.length > 0 ? calls[calls.length - 1][0] : undefined;
    return sent === "custom" ? 6 : fanMode;
  };
  return (
    <>
      <PerformanceCard
        palette={palette}
        snapshot={snapshot(fanMode)}
        onRefresh={() => applyMode(reported)}
        onError={() => {}}
      />
      <div data-testid="curve-slot">
        {!isCustomizeMode(fanMode) ? (
          <CurveHintCard palette={palette} />
        ) : (
          <CurveCard palette={palette} curve={CURVE} writable onApplied={() => {}} />
        )}
      </div>
    </>
  );
}

/** Drive the harness the way a click on the mode button would. */
function renderCard(fanMode: number) {
  render(<Dashboard initialMode={fanMode} />);
}

describe("PerformanceCard", () => {
  beforeEach(() => {
    setFanMode.mockReset().mockResolvedValue(6);
    setPerfMode.mockReset().mockResolvedValue(2);
    setFanCurve.mockReset().mockResolvedValue(undefined);
  });

  it("offers the curve mode under the label customize", () => {
    renderCard(0);
    expect(screen.getByRole("button", { name: /customize/ })).toBeTruthy();
  });

  it("sends the daemon's name (custom) for the customize button", async () => {
    renderCard(0);

    fireEvent.click(screen.getByRole("button", { name: /customize/ }));

    await waitFor(() => expect(setFanMode).toHaveBeenCalledWith("custom"));
  });

  it("marks customize as active when the daemon reports mode 6", () => {
    renderCard(6);
    const button = screen.getByRole("button", { name: /customize/ });
    expect(button.getAttribute("aria-pressed")).toBe("true");
  });

  it("does not mark customize active in other modes", () => {
    renderCard(0);
    const button = screen.getByRole("button", { name: /customize/ });
    expect(button.getAttribute("aria-pressed")).toBe("false");
  });

  it("gives the curve mode its own full-width row", () => {
    // The four presets share a 2x2 grid; customize spans it so it does not read
    // as a fifth preset. Emotion folds `gridColumn` into a generated class, so
    // the placement is asserted through the helper that decides it.
    renderCard(0);
    expect(isFullWidthMode(CUSTOMIZE_FAN_MODE)).toBe(true);
    for (const preset of [0, 1, 5, 8]) {
      expect(isFullWidthMode(preset)).toBe(false);
    }

    // And the rendered cell really is the curve mode's own wrapper.
    const button = screen.getByRole("button", { name: /customize/ });
    const cell = button.parentElement as HTMLElement;
    expect(cell).not.toBe(button);
    expect(cell.className).not.toBe("");
  });
});

describe("curve editor gate", () => {
  beforeEach(() => {
    setFanMode.mockReset().mockResolvedValue(6);
  });

  it("hides the editor and shows the hint in a non-customize mode", () => {
    renderCard(0);
    expect(screen.queryByRole("button", { name: "应用曲线" })).toBeNull();
    expect(screen.getByText(/其他模式下固件不使用自定义曲线/)).toBeTruthy();
  });

  it("shows the editor in customize mode", () => {
    renderCard(6);
    expect(screen.getByRole("button", { name: "应用曲线" })).toBeTruthy();
    expect(screen.queryByText(/其他模式下固件不使用自定义曲线/)).toBeNull();
  });

  it("swaps the hint for the editor after switching to customize", async () => {
    renderCard(0);
    // Starts without the editor.
    expect(screen.queryByRole("button", { name: "应用曲线" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: /customize/ }));

    // The click wrote the mode; the refresh re-renders the mode as customize.
    await waitFor(() => expect(setFanMode).toHaveBeenCalledWith("custom"));
    // In the harness the snapshot follows the click, mirroring the poll.
    expect(await screen.findByRole("button", { name: "应用曲线" })).toBeTruthy();
  });
});
