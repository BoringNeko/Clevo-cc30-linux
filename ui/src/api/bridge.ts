// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Unified backend bridge.
//
// The same frontend bundle is used by two shells:
//
//   * Tauri 2  — `invoke()` from `@tauri-apps/api/core` over the native IPC.
//   * Electron — the preload exposes `window.__CLEVO_ELECTRON__`, whose
//                `invoke()` round-trips to the Rust backend's localhost HTTP
//                bridge (see `ui/electron/main.cjs`).
//
// `invokeBridge()` hides the difference: it prefers the Tauri IPC when it is
// available and falls back to the Electron bridge. The window-control helpers
// (`windowBridge`) do the same for the custom title bar.
//
// Why not branch on `navigator.userAgent`: both shells can be present in a dev
// session, and a plain browser has neither. Feature detection is the reliable
// discriminator and keeps `vite dev` working with no backend at all.

/** The Electron preload's bridge shape. */
interface ElectronBridge {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  window: {
    minimize(): Promise<void>;
    setFullscreen(value: boolean): Promise<void>;
    isFullscreen(): Promise<boolean>;
    setSize(width: number, height: number): Promise<void>;
    hide(): Promise<void>;
    show(): Promise<void>;
    close(): Promise<void>;
    onResized(handler: () => void): (() => void) | Promise<() => void>;
  };
  quit(): Promise<void>;
}

declare global {
  interface Window {
    __CLEVO_ELECTRON__?: ElectronBridge;
  }
}

/** Whether the Electron preload bridge is present. */
export function isElectron(): boolean {
  return typeof window !== "undefined" && !!window.__CLEVO_ELECTRON__;
}

async function tauriInvoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(command, args);
}

/**
 * Call a backend command, resolved by whichever shell is hosting the page.
 *
 * Throws if neither shell is available (plain browser dev), matching the Tauri
 * `invoke` contract so existing `try/catch` fallbacks keep working.
 */
export async function invokeBridge<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const electron = typeof window !== "undefined" ? window.__CLEVO_ELECTRON__ : undefined;
  if (electron) return electron.invoke<T>(command, args);
  return tauriInvoke<T>(command, args);
}

/** The subset of window controls the custom title bar needs. */
export interface WindowControlsBridge {
  minimize(): Promise<void>;
  setFullscreen(value: boolean): Promise<void>;
  isFullscreen(): Promise<boolean>;
  setSize(width: number, height: number): Promise<void>;
  hide(): Promise<void>;
  close(): Promise<void>;
  onResized(handler: () => void): Promise<() => void> | (() => void);
}

/**
 * Return the window-control bridge for the current shell, or null in a plain
 * browser (where the controls become no-ops).
 */
export async function windowBridge(): Promise<WindowControlsBridge | null> {
  const electron = typeof window !== "undefined" ? window.__CLEVO_ELECTRON__ : undefined;
  if (electron) return electron.window;
  try {
    const { getCurrentWindow, LogicalSize } = await import("@tauri-apps/api/window");
    const win = getCurrentWindow();
    return {
      minimize: () => win.minimize(),
      setFullscreen: (value) => win.setFullscreen(value),
      isFullscreen: () => win.isFullscreen(),
      setSize: (width, height) => win.setSize(new LogicalSize(width, height)),
      hide: () => win.hide(),
      close: () => win.close(),
      onResized: (handler) => win.onResized(handler),
    };
  } catch {
    return null;
  }
}

/** Quit the whole application (tray included) in whichever shell is hosting. */
export async function quitBridge(): Promise<void> {
  if (isElectron()) {
    await window.__CLEVO_ELECTRON__!.quit();
    return;
  }
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("quit_app");
}
