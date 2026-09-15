import { describe, expect, it, vi, beforeEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { useWindowSize } from "./useWindowSize";
import { DEFAULT_APPEARANCE, type Appearance } from "../theme";

const setSize = vi.fn().mockResolvedValue(undefined);

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ setSize }),
  LogicalSize: class {
    constructor(
      public width: number,
      public height: number,
    ) {}
  },
}));

describe("useWindowSize", () => {
  beforeEach(() => {
    setSize.mockClear();
  });

  it("applies the persisted size on startup", async () => {
    const appearance: Appearance = { ...DEFAULT_APPEARANCE, displayWidth: 1920, displayHeight: 1080 };
    renderHook(() => useWindowSize(appearance));
    await waitFor(() => expect(setSize).toHaveBeenCalledTimes(1));
    const size = setSize.mock.calls[0][0] as { width: number; height: number };
    expect(size.width).toBe(1920);
    expect(size.height).toBe(1080);
  });

  it("reapplies when the resolution changes", async () => {
    const { rerender } = renderHook(({ a }) => useWindowSize(a), {
      initialProps: { a: { ...DEFAULT_APPEARANCE, displayWidth: 1280, displayHeight: 720 } },
    });
    await waitFor(() => expect(setSize).toHaveBeenCalledTimes(1));
    rerender({ a: { ...DEFAULT_APPEARANCE, displayWidth: 2560, displayHeight: 1440 } });
    await waitFor(() => expect(setSize).toHaveBeenCalledTimes(2));
    const size = setSize.mock.calls[1][0] as { width: number; height: number };
    expect(size.width).toBe(2560);
    expect(size.height).toBe(1440);
  });

  it("returns a resize function for manual use", async () => {
    const { result } = renderHook(() => useWindowSize(DEFAULT_APPEARANCE));
    await waitFor(() => expect(setSize).toHaveBeenCalled());
    result.current(1600, 900);
    await waitFor(() => {
      const calls = setSize.mock.calls;
      const last = calls[calls.length - 1]?.[0] as { width: number; height: number };
      expect(last).toMatchObject({ width: 1600, height: 900 });
    });
  });
});
