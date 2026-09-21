import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { WindowControls } from "./WindowControls";

const state = {
  minimize: vi.fn(),
  setFullscreen: vi.fn(),
  close: vi.fn(),
  isFullscreen: vi.fn(),
};

const hideMainWindow = vi.fn();

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    minimize: state.minimize,
    setFullscreen: state.setFullscreen,
    close: state.close,
    isFullscreen: state.isFullscreen,
    onResized: async () => () => {},
  }),
}));

vi.mock("../api/daemon", () => ({
  hideMainWindow: () => hideMainWindow(),
}));

/** Wait until the window handle has loaded into the component. */
async function ready() {
  await waitFor(() => expect(state.isFullscreen).toHaveBeenCalled());
}

describe("WindowControls", () => {
  beforeEach(() => {
    state.minimize.mockReset().mockResolvedValue(undefined);
    state.setFullscreen.mockReset().mockResolvedValue(undefined);
    state.close.mockReset().mockResolvedValue(undefined);
    state.isFullscreen.mockReset().mockResolvedValue(false);
    hideMainWindow.mockReset().mockResolvedValue(undefined);
  });

  it("renders minimise, fullscreen and close controls", () => {
    render(<WindowControls />);
    expect(screen.getByRole("button", { name: "最小化" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "全屏" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "关闭到托盘" })).toBeTruthy();
  });

  it("minimises the window", async () => {
    render(<WindowControls />);
    await ready();
    fireEvent.click(screen.getByRole("button", { name: "最小化" }));
    await waitFor(() => expect(state.minimize).toHaveBeenCalledTimes(1));
  });

  it("enters fullscreen", async () => {
    render(<WindowControls />);
    await ready();
    fireEvent.click(screen.getByRole("button", { name: "全屏" }));
    await waitFor(() => expect(state.setFullscreen).toHaveBeenCalledWith(true));
  });

  /// Closing must hide to the tray, not quit: the app keeps the tray's fan and
  /// performance controls alive.
  it("hides to the tray instead of closing", async () => {
    render(<WindowControls />);
    await ready();
    fireEvent.click(screen.getByRole("button", { name: "关闭到托盘" }));
    await waitFor(() => expect(hideMainWindow).toHaveBeenCalledTimes(1));
    expect(state.close).not.toHaveBeenCalled();
  });

  /// If hiding is unavailable, fall back to closing the window; the Rust side
  /// intercepts that close request too, so the app still stays in the tray.
  it("falls back to closing when hiding fails", async () => {
    hideMainWindow.mockRejectedValue(new Error("no command"));
    render(<WindowControls />);
    await ready();
    fireEvent.click(screen.getByRole("button", { name: "关闭到托盘" }));
    await waitFor(() => expect(state.close).toHaveBeenCalledTimes(1));
  });
});
