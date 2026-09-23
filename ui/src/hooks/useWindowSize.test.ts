import { describe, expect, it, vi, beforeEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { useWindowSize } from "./useWindowSize";
import { DEFAULT_APPEARANCE, type Appearance } from "../theme";

const setSize = vi.fn().mockResolvedValue(undefined);

// The hook resizes through the shell-agnostic window bridge now.
vi.mock("../api/bridge", () => ({
  windowBridge: async () => ({
    setSize: (width: number, height: number) => setSize(width, height),
  }),
}));

describe("useWindowSize", () => {
  beforeEach(() => {
    setSize.mockClear();
  });

  it("applies the persisted size on startup", async () => {
    const appearance: Appearance = { ...DEFAULT_APPEARANCE, displayWidth: 1920, displayHeight: 1080 };
    renderHook(() => useWindowSize(appearance));
    await waitFor(() => expect(setSize).toHaveBeenCalledTimes(1));
    expect(setSize).toHaveBeenCalledWith(1920, 1080);
  });

  it("reapplies when the resolution changes", async () => {
    const { rerender } = renderHook(({ a }) => useWindowSize(a), {
      initialProps: { a: { ...DEFAULT_APPEARANCE, displayWidth: 1280, displayHeight: 720 } },
    });
    await waitFor(() => expect(setSize).toHaveBeenCalledTimes(1));
    rerender({ a: { ...DEFAULT_APPEARANCE, displayWidth: 2560, displayHeight: 1440 } });
    await waitFor(() => expect(setSize).toHaveBeenCalledTimes(2));
    expect(setSize).toHaveBeenLastCalledWith(2560, 1440);
  });

  it("returns a resize function for manual use", async () => {
    const { result } = renderHook(() => useWindowSize(DEFAULT_APPEARANCE));
    await waitFor(() => expect(setSize).toHaveBeenCalled());
    result.current(1600, 900);
    await waitFor(() => {
      expect(setSize).toHaveBeenLastCalledWith(1600, 900);
    });
  });
});
