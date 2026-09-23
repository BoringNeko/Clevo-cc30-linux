#!/usr/bin/env node
// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Verify the Electron preload exposes exactly the surface the renderer's bridge
// expects, without needing the Electron runtime.
//
// `preload.cjs` runs under Electron's preload environment, where `require("electron")`
// resolves to the Electron module. Here we stub that module, capture what the
// preload passes to `contextBridge.exposeInMainWorld`, and assert the shape.
// This catches a renamed method or a missing window control before packaging.
//
// The expected surface mirrors `ui/src/api/bridge.ts` (ElectronBridge).
//
// Usage: node scripts/tests-electron-preload.mjs
//        (run from the repository root)

import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const require = createRequire(import.meta.url);
const root = join(dirname(fileURLToPath(import.meta.url)), "..");

let exposed = null;
const channels = [];

const Module = require("node:module");
const originalLoad = Module._load;
Module._load = function (request, parent, isMain) {
  if (request === "electron") {
    return {
      contextBridge: {
        exposeInMainWorld: (name, api) => {
          exposed = { name, api };
        },
      },
      ipcRenderer: {
        invoke: (channel) => {
          channels.push(channel);
          return Promise.resolve(undefined);
        },
        on: () => {},
        removeListener: () => {},
      },
    };
  }
  return originalLoad.call(this, request, parent, isMain);
};

require(join(root, "ui/electron/preload.cjs"));

const problems = [];
const check = (cond, message) => {
  if (!cond) problems.push(message);
};

check(exposed !== null, "preload did not call contextBridge.exposeInMainWorld");
if (exposed) {
  check(
    exposed.name === "__CLEVO_ELECTRON__",
    `global is ${exposed.name}, expected __CLEVO_ELECTRON__ (see src/api/bridge.ts)`,
  );
  const api = exposed.api;
  for (const method of ["invoke", "window", "quit"]) {
    check(typeof api[method] !== "undefined", `missing api.${method}`);
  }
  for (const method of [
    "minimize",
    "setFullscreen",
    "isFullscreen",
    "setSize",
    "hide",
    "show",
    "close",
    "onResized",
  ]) {
    check(typeof api.window?.[method] === "function", `missing api.window.${method}`);
  }
  // The invoke channel must match the main process handler in main.cjs.
  check(
    typeof api.invoke === "function",
    "api.invoke is not callable",
  );
}

if (problems.length > 0) {
  for (const p of problems) console.error(`  FAIL ${p}`);
  process.exit(1);
}
console.log("  ok   preload exposes the expected bridge surface");
