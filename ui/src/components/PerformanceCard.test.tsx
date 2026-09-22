import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { PerformanceCard } from "./PerformanceCard";
import type { FanSnapshot } from "../api/daemon";

const setFanMode = vi.fn();
const setPerfMode = vi.fn();

vi.mock("../api/daemon", async () => {
  // Keep the real constants/helpers (the mode table and the predicate) and stub
  // only the D-Bus writes.
  const actual = await vi.importActual<typeof import("../api/daemon")>("../api/daemon");
  return {
    ...actual,
    setFanMode: (mode: string) => setFanMode(mode),
    setPerfMode: (mode: string) => setPerfMode(mode),
  };
});

const palette = {
  primary: [120, 200, 255] as [number, number, number],
  secondary: [180, 120, 255] as [number, number, number],
  swatches: [] as Array<[number, number, number]>,
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

function renderCard(fanMode: number) {
  const onRefresh = vi.fn();
  const onError = vi.fn();
  render(
    <PerformanceCard
      palette={palette}
      snapshot={snapshot(fanMode)}
      onRefresh={onRefresh}
      onError={onError}
    />,
  );
  return { onRefresh, onError };
}

describe("PerformanceCard", () => {
  beforeEach(() => {
    setFanMode.mockReset().mockResolvedValue(6);
    setPerfMode.mockReset().mockResolvedValue(2);
  });

  it("offers the curve mode under the label customize", () => {
    renderCard(0);
    expect(screen.getByRole("button", { name: /customize/ })).toBeTruthy();
  });

  it("sends the daemon's name (custom) for the customize button", async () => {
    const { onRefresh } = renderCard(0);

    fireEvent.click(screen.getByRole("button", { name: /customize/ }));

    await waitFor(() => expect(setFanMode).toHaveBeenCalledWith("custom"));
    // The mode was applied, so the dashboard must re-read the state.
    await waitFor(() => expect(onRefresh).toHaveBeenCalled());
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
});
