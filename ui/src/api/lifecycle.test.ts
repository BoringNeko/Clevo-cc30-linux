// SPDX-License-Identifier: MIT OR Apache-2.0
//
// App-lifecycle routing: the close button must reach the right shell.
//
// Under Electron the window is owned by the main process and closing destroys
// it (releasing the renderer); a backend command would not exist. Under Tauri
// the Rust side owns the window, so the backend command is the correct path.
// This pins that split.

import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";

const invoke = vi.hoisted(() => vi.fn().mockResolvedValue(undefined));
const hide = vi.hoisted(() => vi.fn().mockResolvedValue(undefined));

vi.mock("./bridge", async () => {
  const actual = await vi.importActual<typeof import("./bridge")>("./bridge");
  return {
    ...actual,
    invokeBridge: invoke,
    // Overridden per-test via installElectron()/removeElectron(); the default
    // mirrors a Tauri build (no Electron global).
    isElectron: () => false,
    windowBridge: async () => ({ hide, minimize: vi.fn(), setFullscreen: vi.fn(),
      isFullscreen: vi.fn(), setSize: vi.fn(), close: vi.fn(), onResized: vi.fn() }),
  };
});

import { hideMainWindow } from "./daemon";
import * as bridge from "./bridge";

describe("hideMainWindow", () => {
  beforeEach(() => {
    invoke.mockClear();
    hide.mockClear();
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("uses the backend command outside Electron (Tauri)", async () => {
    vi.spyOn(bridge, "isElectron").mockReturnValue(false);
    await hideMainWindow();
    expect(invoke).toHaveBeenCalledWith("hide_main_window");
    expect(hide).not.toHaveBeenCalled();
  });

  it("uses the window bridge under Electron (destroys the window)", async () => {
    vi.spyOn(bridge, "isElectron").mockReturnValue(true);
    vi.spyOn(bridge, "windowBridge").mockResolvedValue({
      hide,
      minimize: vi.fn(),
      setFullscreen: vi.fn(),
      isFullscreen: vi.fn(),
      setSize: vi.fn(),
      close: vi.fn(),
      onResized: vi.fn(),
    } as unknown as bridge.WindowControlsBridge);
    await hideMainWindow();
    expect(hide).toHaveBeenCalled();
    expect(invoke).not.toHaveBeenCalled();
  });
});
