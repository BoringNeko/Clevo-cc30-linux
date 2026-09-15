import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { WindowControls } from "./WindowControls";

const state = {
  minimize: vi.fn(),
  setFullscreen: vi.fn(),
  close: vi.fn(),
  isFullscreen: vi.fn(),
};

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    minimize: state.minimize,
    setFullscreen: state.setFullscreen,
    close: state.close,
    isFullscreen: state.isFullscreen,
    onResized: async () => () => {},
  }),
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
  });

  it("renders minimise, fullscreen and close controls", () => {
    render(<WindowControls />);
    expect(screen.getByRole("button", { name: "最小化" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "全屏" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "关闭" })).toBeTruthy();
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

  it("closes the window", async () => {
    render(<WindowControls />);
    await ready();
    fireEvent.click(screen.getByRole("button", { name: "关闭" }));
    await waitFor(() => expect(state.close).toHaveBeenCalledTimes(1));
  });
});
