#!/usr/bin/env node
// SPDX-License-Identifier: MIT OR Apache-2.0
//
// clevo-cc-ui (Electron backend shell)
// ====================================
//
// This is a *thin shell*: it reuses the existing Rust backend binary
// (`clevo-cc-ui`, the Tauri crate built with `--no-default-features
// --backend-only`) as a child process and speaks the same invoke protocol the
// Tauri webview uses, but over a dedicated localhost HTTP port so it works with
// Electron's Chromium renderer.
//
// Why a shell instead of reimplementing the backend in Node: the backend owns
// all the D-Bus / PolicyKit / asset logic and is covered by Rust tests. Node
// only needs to (a) start the process, (b) bridge `invoke()` calls to it, and
// (c) provide the native window, tray icon and custom title-bar controls.
//
// The XHR fallback in `src/api/tauri-bridge.ts` is what makes the *same*
// frontend bundle work under both Tauri (IPC) and Electron (this HTTP bridge).

"use strict";

const { app, BrowserWindow, Tray, Menu, ipcMain, nativeImage, screen, shell } = require("electron");
const { spawn } = require("node:child_process");
const http = require("node:http");
const fs = require("node:fs");
const path = require("node:path");

// Backend binary: bundled next to the app resources when packaged, otherwise
// the headless build from the Tauri crate (`target/electron/release`, made with
// `cargo build --no-default-features`), then the ordinary Tauri build.
function resolveBackendPath() {
  const env = process.env.CLEVO_CC_UI_BACKEND;
  if (env && fs.existsSync(env)) return env;
  const tauriDir = path.join(__dirname, "..", "src-tauri", "target");
  const candidates = [
    path.join(process.resourcesPath || "", "clevo-cc-ui"),
    path.join(tauriDir, "electron", "release", "clevo-cc-ui"),
    path.join(tauriDir, "release", "clevo-cc-ui"),
    path.join(tauriDir, "debug", "clevo-cc-ui"),
  ];
  for (const c of candidates) {
    try {
      if (c && fs.existsSync(c)) return c;
    } catch {
      /* ignore */
    }
  }
  return null;
}

// Where the frontend's static bundle lives.
function resolveFrontendDir() {
  const env = process.env.CLEVO_CC_UI_DIST;
  if (env && fs.existsSync(path.join(env, "index.html"))) return env;
  const dist = path.join(__dirname, "..", "dist");
  if (fs.existsSync(path.join(dist, "index.html"))) return dist;
  return null;
}

// Locate the app icon. When packaged, electron-builder copies `icon.png` into
// resources (see package.json extraResources); in a source checkout the Tauri
// icon directory is used.
function resolveIcon(name) {
  const candidates = [
    path.join(process.resourcesPath || "", name),
    path.join(__dirname, "..", "src-tauri", "icons", name),
  ];
  return candidates.find((p) => p && fs.existsSync(p)) || null;
}

let backend = null; // child process
let backendPort = 0;
let backendToken = ""; // shared secret so only our renderer can call the bridge
let mainWindow = null;
let tray = null;
let quitting = false;

// Parse the child's handshake line: `CLEVO_CC_BACKEND <port> <token>`.
function waitForHandshake(child) {
  return new Promise((resolve, reject) => {
    let buf = "";
    const onData = (chunk) => {
      buf += chunk.toString("utf8");
      const line = buf.split("\n").find((l) => l.startsWith("CLEVO_CC_BACKEND"));
      if (line) {
        const [, port, token] = line.trim().split(/\s+/);
        child.stdout.off("data", onData);
        resolve({ port: Number(port), token: token || "" });
      }
    };
    child.stdout.on("data", onData);
    child.once("error", reject);
    child.once("exit", (code) => reject(new Error(`backend exited early (code ${code})`)));
    setTimeout(() => reject(new Error("backend handshake timed out")), 15000).unref();
  });
}

async function startBackend() {
  const backendPath = resolveBackendPath();
  if (!backendPath) {
    throw new Error("clevo-cc-ui backend binary not found (set CLEVO_CC_UI_BACKEND)");
  }
  backend = spawn(backendPath, ["--serve"], {
    stdio: ["ignore", "pipe", "inherit"],
    env: process.env,
  });
  const { port, token } = await waitForHandshake(backend);
  backendPort = port;
  backendToken = token;
  backend.on("exit", () => {
    if (!quitting) {
      // The backend is the only thing that talks to hardware; without it the
      // UI has nothing to show. Exit with it rather than lingering.
      app.quit();
    }
  });
}

// Proxy a renderer `invoke` to the backend's HTTP bridge.
function bridgeInvoke(command, args) {
  return new Promise((resolve, reject) => {
    const payload = JSON.stringify({ command, args: args ?? {} });
    const req = http.request(
      {
        host: "127.0.0.1",
        port: backendPort,
        path: "/invoke",
        method: "POST",
        headers: {
          "content-type": "application/json",
          "content-length": Buffer.byteLength(payload),
          "x-clevo-token": backendToken,
        },
      },
      (res) => {
        let body = "";
        res.setEncoding("utf8");
        res.on("data", (c) => (body += c));
        res.on("end", () => {
          try {
            const parsed = JSON.parse(body || "{}");
            if (parsed.ok) resolve(parsed.value);
            else reject(new Error(parsed.error || "backend error"));
          } catch (e) {
            reject(new Error(`bad backend reply: ${e.message}`));
          }
        });
      },
    );
    req.on("error", (e) => reject(new Error(`backend unreachable: ${e.message}`)));
    req.end(payload);
  });
}

ipcMain.handle("clevo:invoke", (_event, command, args) => bridgeInvoke(command, args));

// --- window state -----------------------------------------------------------

// The persisted window size is driven from the frontend through the
// `set_window_size` invoke, which the backend translates into a callback the
// shell consumes. Simpler: expose dedicated window IPC the preload uses.
ipcMain.handle("clevo:window", (event, action, value) => {
  // "hide" is the title bar's close button. It destroys the window (releasing
  // the renderer) rather than merely hiding it, matching the close handler; the
  // tray's "open" recreates it. The other actions operate on the sender window
  // while it exists.
  if (action === "hide") {
    destroyWindow();
    return null;
  }
  if (action === "show") {
    showWindow();
    return null;
  }

  const win = BrowserWindow.fromWebContents(event.sender);
  if (!win) return null;
  switch (action) {
    case "minimize":
      win.minimize();
      return null;
    case "toggle-fullscreen":
      win.setFullScreen(!win.isFullScreen());
      return null;
    case "set-fullscreen":
      win.setFullScreen(Boolean(value));
      return null;
    case "is-fullscreen":
      return win.isFullScreen();
    case "set-size": {
      const [w, h] = value || [];
      if (w > 0 && h > 0) win.setContentSize(Math.round(w), Math.round(h));
      return null;
    }
    case "close":
      win.close();
      return null;
    default:
      return null;
  }
});

ipcMain.handle("clevo:quit", () => {
  quitting = true;
  app.quit();
});

// --- tray -------------------------------------------------------------------
//
// Equivalent to the Tauri tray (src-tauri/src/tray.rs): a performance-mode
// submenu that talks to the daemon through the backend, plus open/quit. The
// active mode is marked with the same `▶` / `・` prefixes.

const PERF_MODES = [
  { label: "静音", name: "quiet", value: 0 },
  { label: "节能", name: "pwrsaving", value: 1 },
  { label: "性能", name: "performance", value: 2 },
  { label: "娱乐", name: "entertainment", value: 3 },
];
const ACTIVE_MARK = "▶ ";
const INACTIVE_MARK = "・ ";

async function currentPerfMode() {
  try {
    const snap = await bridgeInvoke("get_fan_snapshot", {});
    const v = snap && snap.perf_mode;
    return v < PERF_MODES.length ? v : null;
  } catch {
    return null;
  }
}

async function applyPerfMode(value) {
  const mode = PERF_MODES.find((m) => m.value === value);
  if (!mode) return;
  try {
    await bridgeInvoke("set_perf_mode", { mode: mode.name });
  } catch (e) {
    console.error("tray: could not set performance mode:", e.message);
  }
  await refreshTray();
}

async function refreshTray() {
  if (!tray) return;
  const active = await currentPerfMode();
  const template = [
    {
      label: "性能模式",
      submenu: PERF_MODES.map((m) => ({
        label: (active === m.value ? ACTIVE_MARK : INACTIVE_MARK) + m.label,
        click: () => applyPerfMode(m.value),
      })),
    },
    { type: "separator" },
    {
      label: "打开控制中心",
      click: () => showWindow(),
    },
    {
      label: "退出",
      click: () => {
        quitting = true;
        app.quit();
      },
    },
  ];
  tray.setContextMenu(Menu.buildFromTemplate(template));
}

function buildTray() {
  const iconPath = resolveIcon("icon.png") || resolveIcon("32x32.png");
  const image = iconPath ? nativeImage.createFromPath(iconPath) : nativeImage.createEmpty();
  tray = new Tray(image);
  tray.setToolTip("Clevo Control Center");
  void refreshTray();
  // Left click: open the window (matches the Tauri behaviour).
  tray.on("click", () => showWindow());
}

// --- window -----------------------------------------------------------------
//
// Closing the window destroys it (and with it the whole Chromium renderer
// process tree) but keeps the app and tray alive. This is the deliberate
// difference from the Tauri build, whose WebKit web process is torn down by the
// toolkit anyway: a hidden Electron window would keep ~600 MB of renderer,
// GPU and utility processes resident for the whole tray session. Recreating the
// window on demand costs a re-render, which is cheap next to that.
//
// The backend (`--serve`) is *not* restarted: it keeps the D-Bus connection and
// the cached fan state, and the recreated renderer fetches a fresh snapshot as
// part of its normal startup (see src/hooks/useAppState / api/daemon).

function createWindow() {
  if (mainWindow && !mainWindow.isDestroyed()) {
    mainWindow.show();
    mainWindow.focus();
    return mainWindow;
  }

  mainWindow = new BrowserWindow({
    width: 1600,
    height: 900,
    minWidth: 568,
    minHeight: 320,
    frame: false, // custom title bar, same as the Tauri config
    backgroundColor: "#000000",
    icon: resolveIcon("icon.png") || resolveIcon("128x128.png") || undefined,
    webPreferences: {
      preload: path.join(__dirname, "preload.cjs"),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false,
    },
  });

  const frontendDir = resolveFrontendDir();
  if (frontendDir) {
    const index = path.join(frontendDir, "index.html");
    mainWindow.loadFile(index);
  } else {
    // Dev: the Vite server.
    const url = process.env.CLEVO_CC_UI_DEV_URL || "http://localhost:1420";
    mainWindow.loadURL(url);
  }

  // Closing must not quit the app: the tray keeps the fan and performance
  // controls available. Unlike a plain `hide()`, the window is destroyed so the
  // renderer memory is released; the tray's "open" recreates it.
  mainWindow.on("close", (event) => {
    if (!quitting) {
      event.preventDefault();
      destroyWindow();
    }
  });
  mainWindow.on("closed", () => {
    mainWindow = null;
  });
  // Push resize/fullscreen changes to the renderer so the title bar's
  // fullscreen icon tracks the window state (the Tauri build gets this from
  // `onResized` too).
  const notifyResized = () => {
    if (mainWindow && !mainWindow.isDestroyed()) {
      mainWindow.webContents.send("clevo:window-resized");
    }
  };
  mainWindow.on("resize", notifyResized);
  mainWindow.on("enter-full-screen", notifyResized);
  mainWindow.on("leave-full-screen", notifyResized);

  // Keep external links out of the app frame.
  mainWindow.webContents.setWindowOpenHandler(({ url }) => {
    void shell.openExternal(url);
    return { action: "deny" };
  });

  // Test hook (no effect in normal use): after the window is destroyed, reopen
  // it once after a delay, so scripts/electron-tray-release.sh can verify the
  // tray's recreate path without a real tray click. Active only when
  // CLEVO_CC_E2E_REOPEN_MS is set.
  const reopenMs = Number(process.env.CLEVO_CC_E2E_REOPEN_MS || 0);
  if (reopenMs > 0) {
    mainWindow.on("closed", () => {
      setTimeout(() => {
        if (!quitting) showWindow();
      }, reopenMs);
    });
  }

  return mainWindow;
}

// Destroy the window, releasing the renderer/GPU processes. The app, tray and
// backend stay up; `createWindow` brings it back.
function destroyWindow() {
  if (mainWindow && !mainWindow.isDestroyed()) {
    // Remove our close handler first: `destroy()` emits `close` on some
    // platforms, and the handler would call back into here.
    mainWindow.removeAllListeners("close");
    mainWindow.destroy();
  }
  mainWindow = null;
}

// Show the window, recreating it if it was destroyed on close.
function showWindow() {
  const win = createWindow();
  win.show();
  win.focus();
}

// Chromium features that map onto the WebKit workarounds documented in
// docs/hardware-notes.md §15. Electron does not use WebKitGTK, so the WebKit
// environment variables are irrelevant here; these are the Chromium equivalents
// that keep the NVIDIA/GBM path from breaking compositing.
function applyChromiumSwitches() {
  // Disable Chromium's own GBM/DMA-BUF path when the user asked for it (the
  // Settings → Compatibility "software rendering" toggle sets this).
  if (process.env.CLEVO_CC_SOFTWARE_RENDERING === "1") {
    app.commandLine.appendSwitch("disable-gpu");
    app.commandLine.appendSwitch("disable-gpu-compositing");
    app.commandLine.appendSwitch("disable-features", "VizDisplayCompositor");
  }
}

app.whenReady().then(async () => {
  applyChromiumSwitches();
  try {
    await startBackend();
  } catch (e) {
    console.error("could not start the clevo-cc backend:", e.message);
    app.quit();
    return;
  }
  createWindow();
  buildTray();

  app.on("activate", () => showWindow());
});

// Closing the last window must not quit: the tray keeps the app alive.
app.on("window-all-closed", () => {
  if (quitting) app.quit();
});

app.on("before-quit", () => {
  quitting = true;
  if (backend && !backend.killed) backend.kill();
});
