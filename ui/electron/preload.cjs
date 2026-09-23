// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Electron preload: expose a minimal, typed bridge to the renderer.
//
// `window.__CLEVO_ELECTRON__` is the only thing the renderer sees; it mirrors
// the shape the frontend's `tauri-bridge` fallback expects. No Node APIs and no
// ipcRenderer leakage reach the page (contextIsolation + explicit allowlist).

"use strict";

const { contextBridge, ipcRenderer } = require("electron");

contextBridge.exposeInMainWorld("__CLEVO_ELECTRON__", {
  /** Invoke a backend command; returns a promise resolving to its value. */
  invoke(command, args) {
    return ipcRenderer.invoke("clevo:invoke", command, args);
  },

  /** Window controls for the custom title bar. */
  window: {
    minimize: () => ipcRenderer.invoke("clevo:window", "minimize"),
    setFullscreen: (value) => ipcRenderer.invoke("clevo:window", "set-fullscreen", value),
    isFullscreen: () => ipcRenderer.invoke("clevo:window", "is-fullscreen"),
    setSize: (width, height) => ipcRenderer.invoke("clevo:window", "set-size", [width, height]),
    hide: () => ipcRenderer.invoke("clevo:window", "hide"),
    show: () => ipcRenderer.invoke("clevo:window", "show"),
    close: () => ipcRenderer.invoke("clevo:window", "close"),
    onResized: (handler) => {
      const listener = () => handler();
      ipcRenderer.on("clevo:window-resized", listener);
      return () => ipcRenderer.removeListener("clevo:window-resized", listener);
    },
  },

  /** Quit the whole application (tray included). */
  quit: () => ipcRenderer.invoke("clevo:quit"),
});
