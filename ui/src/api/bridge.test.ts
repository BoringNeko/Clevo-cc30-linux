// SPDX-License-Identifier: MIT OR Apache-2.0
//
// The unified backend bridge: the same frontend bundle must reach the Rust
// backend under both shells. These tests pin the dispatch rule (Electron first,
// Tauri as the fallback) and the window-control mapping.

import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { invokeBridge, isElectron, windowBridge } from "./bridge";

const tauriInvoke = vi.hoisted(() => vi.fn());
const tauriWindow = vi.hoisted(() => ({
  minimize: vi.fn(),
  setFullscreen: vi.fn(),
  isFullscreen: vi.fn(),
  setSize: vi.fn(),
  hide: vi.fn(),
  close: vi.fn(),
  onResized: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: tauriInvoke }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => tauriWindow,
  LogicalSize: class {
    constructor(
      public width: number,
      public height: number,
    ) {}
  },
}));

/** Install a fake Electron preload bridge and return its spies. */
function installElectron() {
  const invoke = vi.fn().mockResolvedValue("from-electron");
  const win = {
    minimize: vi.fn(),
    setFullscreen: vi.fn(),
    isFullscreen: vi.fn().mockResolvedValue(false),
    setSize: vi.fn(),
    hide: vi.fn(),
    close: vi.fn(),
    onResized: vi.fn(),
  };
  (window as unknown as { __CLEVO_ELECTRON__?: unknown }).__CLEVO_ELECTRON__ = {
    invoke,
    window: win,
    quit: vi.fn(),
  };
  return { invoke, win };
}

function removeElectron() {
  delete (window as unknown as { __CLEVO_ELECTRON__?: unknown }).__CLEVO_ELECTRON__;
}

describe("invokeBridge", () => {
  beforeEach(() => {
    tauriInvoke.mockReset();
    removeElectron();
  });
  afterEach(removeElectron);

  it("uses the Tauri IPC when no Electron bridge is present", async () => {
    tauriInvoke.mockResolvedValue(42);
    const value = await invokeBridge<number>("get_fan_snapshot");
    expect(value).toBe(42);
    expect(tauriInvoke).toHaveBeenCalledWith("get_fan_snapshot", undefined);
  });

  it("prefers the Electron bridge when it is present", async () => {
    const { invoke } = installElectron();
    tauriInvoke.mockResolvedValue(1);
    const value = await invokeBridge<string>("get_fan_snapshot", { a: 1 });
    expect(value).toBe("from-electron");
    expect(invoke).toHaveBeenCalledWith("get_fan_snapshot", { a: 1 });
    expect(tauriInvoke).not.toHaveBeenCalled();
  });

  it("reports the shell correctly", () => {
    expect(isElectron()).toBe(false);
    installElectron();
    expect(isElectron()).toBe(true);
  });
});

describe("windowBridge", () => {
  beforeEach(() => {
    removeElectron();
    Object.values(tauriWindow).forEach((fn) => fn.mockReset());
    tauriWindow.isFullscreen.mockResolvedValue(true);
  });
  afterEach(removeElectron);

  it("maps onto the Tauri window outside Electron", async () => {
    const bridge = await windowBridge();
    expect(bridge).not.toBeNull();
    await bridge!.setFullscreen(true);
    expect(tauriWindow.setFullscreen).toHaveBeenCalledWith(true);
    expect(await bridge!.isFullscreen()).toBe(true);
  });

  it("maps onto the Electron bridge when present", async () => {
    const { win } = installElectron();
    const bridge = await windowBridge();
    await bridge!.minimize();
    expect(win.minimize).toHaveBeenCalled();
    expect(tauriWindow.minimize).not.toHaveBeenCalled();
  });
});
