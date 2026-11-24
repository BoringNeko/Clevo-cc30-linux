import { act, cleanup, renderHook } from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { isWindowVisible } from "../api/bridge";
import { pollFan, type FanSnapshot } from "../api/daemon";
import { useFanRefresh } from "./useFanRefresh";

vi.mock("../api/bridge", () => ({ isWindowVisible: vi.fn() }));
vi.mock("../api/daemon", () => ({ pollFan: vi.fn() }));

const snapshot: FanSnapshot = {
  cpu: { rpm: 2500, temp_c: 50, available: true },
  gpu1: { rpm: 0, temp_c: null, available: false },
  gpu2: { rpm: 0, temp_c: null, available: false },
  freshness: "fresh",
  fan_count: 1,
  fan_mode: 0,
  perf_mode: 0,
  writable: true,
  curve_writable: true,
};

const advance = (ms: number) => act(() => vi.advanceTimersByTimeAsync(ms));

function setup(enabled = true) {
  const onSnapshot = vi.fn();
  const onError = vi.fn();
  const hook = renderHook(({ enabled }) => useFanRefresh(enabled, onSnapshot, onError), {
    initialProps: { enabled },
    wrapper: StrictMode,
  });
  return { ...hook, onSnapshot, onError };
}

describe("useFanRefresh", () => {
  let hidden: boolean;

  beforeEach(() => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "performance"] });
    hidden = false;
    vi.spyOn(document, "hidden", "get").mockImplementation(() => hidden);
    vi.mocked(isWindowVisible).mockReset().mockResolvedValue(true);
    vi.mocked(pollFan).mockReset().mockResolvedValue(snapshot);
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it("refreshes immediately and then at one-minute intervals without clicking", async () => {
    const { onSnapshot } = setup();
    await advance(0);
    expect(pollFan).toHaveBeenCalledTimes(1);
    expect(onSnapshot).toHaveBeenCalledWith(snapshot);
    await advance(59_999);
    expect(pollFan).toHaveBeenCalledTimes(1);
    await advance(1);
    expect(pollFan).toHaveBeenCalledTimes(2);
    await advance(60_000);
    expect(pollFan).toHaveBeenCalledTimes(3);
  });

  it("stops in a hidden document, then resumes when visible", async () => {
    setup();
    await advance(0);
    hidden = true;
    document.dispatchEvent(new Event("visibilitychange"));
    await advance(120_000);
    expect(pollFan).toHaveBeenCalledTimes(1);
    hidden = false;
    document.dispatchEvent(new Event("visibilitychange"));
    await advance(0);
    expect(pollFan).toHaveBeenCalledTimes(2);
  });

  it("skips a hidden native window even without a document event and resumes on focus", async () => {
    setup();
    await advance(0);
    vi.mocked(isWindowVisible).mockResolvedValue(false);
    await advance(120_000);
    expect(pollFan).toHaveBeenCalledTimes(1);
    vi.mocked(isWindowVisible).mockResolvedValue(true);
    window.dispatchEvent(new Event("focus"));
    await advance(0);
    expect(pollFan).toHaveBeenCalledTimes(2);
  });

  it("stops outside the telemetry pages and when unmounted", async () => {
    const { rerender, unmount } = setup(false);
    await advance(60_000);
    expect(pollFan).not.toHaveBeenCalled();
    rerender({ enabled: true });
    await advance(0);
    expect(pollFan).toHaveBeenCalledTimes(1);
    rerender({ enabled: false });
    await advance(120_000);
    expect(pollFan).toHaveBeenCalledTimes(1);
    rerender({ enabled: true });
    await advance(0);
    expect(pollFan).toHaveBeenCalledTimes(2);
    unmount();
    await advance(120_000);
    expect(pollFan).toHaveBeenCalledTimes(2);
  });

  it("shares an in-flight poll across page changes and waits a minute even if it fails", async () => {
    let rejectPoll!: (error: Error) => void;
    vi.mocked(pollFan).mockReturnValueOnce(new Promise((_resolve, reject) => {
      rejectPoll = reject;
    }));
    const { rerender, onError } = setup();
    await advance(0);
    rerender({ enabled: false });
    rerender({ enabled: true });
    await advance(120_000);
    expect(pollFan).toHaveBeenCalledTimes(1);
    await act(async () => rejectPoll(new Error("failed")));
    expect(onError).toHaveBeenLastCalledWith("Error: failed");
    await advance(59_999);
    expect(pollFan).toHaveBeenCalledTimes(1);
    await advance(1);
    expect(pollFan).toHaveBeenCalledTimes(2);
  });
});
