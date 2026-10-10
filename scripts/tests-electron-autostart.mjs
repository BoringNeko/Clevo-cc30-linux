#!/usr/bin/env node
// Exercise the real Electron entry point without launching a desktop session.
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const source = fs.readFileSync(path.join(root, "ui/electron/main.cjs"), "utf8");

async function launch({ tray = false, packaged = true, appImage, lock = true } = {}) {
  const windows = [];
  const trays = [];
  const spawns = [];
  const app = new EventEmitter();
  Object.assign(app, {
    isPackaged: packaged,
    requestSingleInstanceLock: () => lock,
    whenReady: () => Promise.resolve(),
    quit: () => {},
    commandLine: { appendSwitch: () => {} },
  });
  class Window extends EventEmitter {
    constructor() {
      super();
      this.webContents = { setWindowOpenHandler: () => {}, send: () => {} };
      this.destroyed = false;
      windows.push(this);
    }
    loadFile() {}
    loadURL() {}
    show() {}
    focus() {}
    isDestroyed() { return this.destroyed; }
    destroy() { this.destroyed = true; this.emit("closed"); }
  }
  class Tray extends EventEmitter {
    constructor() { super(); trays.push(this); }
    setToolTip() {}
    setContextMenu(menu) { this.menu = menu; }
  }
  const electron = {
    app, BrowserWindow: Window, Tray,
    Menu: { buildFromTemplate: (template) => template },
    ipcMain: { handle: () => {} },
    nativeImage: { createFromPath: () => ({}), createEmpty: () => ({}) },
  };
  const modules = {
    electron,
    "node:path": path,
    "node:fs": { existsSync: () => true },
    "node:child_process": {
      spawn: (exe, args, opts) => {
        spawns.push({ exe, args, opts });
        const child = new EventEmitter();
        child.stdout = new EventEmitter();
        setImmediate(() => child.stdout.emit("data", "CLEVO_CC_BACKEND 9999 token\n"));
        return child;
      },
    },
    "node:http": {
      request: (_opts, receive) => {
        const req = new EventEmitter();
        req.end = () => {
          const res = new EventEmitter();
          res.setEncoding = () => {};
          receive(res);
          res.emit("data", JSON.stringify({ ok: true, value: { perf_mode: 2 } }));
          res.emit("end");
        };
        return req;
      },
    },
  };
  vm.runInNewContext(source, {
    require: (name) => {
      assert.ok(name in modules, `unexpected require: ${name}`);
      return modules[name];
    },
    __dirname: path.join(root, "ui/electron"),
    process: {
      argv: ["electron", "app", ...(tray ? ["--tray"] : [])],
      execPath: "/usr/lib/clevo-cc-ui-electron/clevo-cc-ui-electron",
      env: appImage ? { APPIMAGE: appImage } : {},
      resourcesPath: "/resources",
    },
    Buffer,
    console,
    setTimeout: () => ({ unref() {} }),
  });
  await new Promise((resolve) => setImmediate(resolve));
  await new Promise((resolve) => setImmediate(resolve));
  return { app, windows, trays, spawns };
}

const resident = await launch({ tray: true });
assert.equal(resident.windows.length, 0, "tray startup must not create a window");
assert.equal(resident.trays.length, 1);
assert.equal(resident.spawns.length, 1);
resident.app.emit("second-instance", {}, ["electron", "--tray"]);
assert.equal(resident.windows.length, 0, "another autostart must not open a window");
resident.trays[0].menu.find((item) => item.label === "打开控制中心").click();
assert.equal(resident.windows.length, 1, "tray menu must create the first window");
resident.windows[0].emit("close", { preventDefault() {} });
assert.equal(resident.windows[0].destroyed, true);
resident.app.emit("second-instance", {}, ["electron"]);
assert.equal(resident.windows.length, 2, "manual launch must reopen the resident app");
assert.equal(resident.spawns.length, 1, "reopening must keep the backend");
assert.equal(JSON.parse(resident.spawns[0].opts.env.CLEVO_CC_UI_LAUNCHER).executable,
  "/usr/lib/clevo-cc-ui-electron/clevo-cc-ui-electron");

const normal = await launch({ packaged: false });
assert.equal(normal.windows.length, 1, "manual startup must show the window");
assert.deepEqual(JSON.parse(normal.spawns[0].opts.env.CLEVO_CC_UI_LAUNCHER).args,
  [path.join(root, "ui")]);
const image = await launch({ appImage: "/home/user/Control Center.AppImage" });
assert.deepEqual(JSON.parse(image.spawns[0].opts.env.CLEVO_CC_UI_LAUNCHER), {
  executable: "/home/user/Control Center.AppImage", args: [],
});
const duplicate = await launch({ lock: false });
assert.equal(duplicate.spawns.length, 0);
assert.equal(duplicate.windows.length, 0);
assert.equal(duplicate.trays.length, 0);
console.log("Electron autostart, tray startup, reopen and launcher checks passed");
