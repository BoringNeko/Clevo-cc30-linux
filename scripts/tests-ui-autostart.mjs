#!/usr/bin/env node
// Verify the actual backend bridge and desktop-entry quoting in a temp config.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const binary = process.argv[2] || path.join(root, "ui/src-tauri/target/debug/clevo-cc-ui");
const dir = await fs.mkdtemp(path.join(os.tmpdir(), "clevo-autostart-e2e-"));
const config = path.join(dir, "config");
const entry = path.join(config, "autostart/org.clevo.cc.ui.desktop");
const probe = path.join(dir, "Probe a%f$`\"\\ UI");
const probeOutput = path.join(dir, "argv");
const args = ["/source UI", "quote%f\"$`\\end"];
await fs.writeFile(probe, '#!/bin/sh\nprintf "%s\\n" "$@" > "$CLEVO_AUTOSTART_PROBE"\n', { mode: 0o755 });
let child;

async function stop() {
  if (child && child.exitCode === null) {
    const exited = once(child, "exit");
    child.kill();
    await exited;
  }
  child = null;
}

async function start(launcher) {
  child = spawn(binary, ["--serve"], {
    env: {
      ...process.env, XDG_CONFIG_HOME: config,
      CLEVO_CC_UI_LAUNCHER: JSON.stringify(launcher),
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stderr = "";
  child.stderr.on("data", (chunk) => { stderr += chunk.toString(); });
  const lines = createInterface({ input: child.stdout });
  const handshake = await Promise.race([
    once(lines, "line"),
    once(child, "exit").then(([code]) => { throw new Error(`backend exited with ${code}: ${stderr}`); }),
    new Promise((_, reject) => setTimeout(() => reject(new Error("backend handshake timed out")), 10000).unref()),
  ]);
  const [, port, token] = handshake[0].split(" ");
  assert.ok(port && token, `invalid handshake: ${handshake}`);
  return async (command, args = {}) => {
    const reply = await fetch(`http://127.0.0.1:${port}/invoke`, {
      method: "POST",
      headers: { "content-type": "application/json", "x-clevo-token": token },
      body: JSON.stringify({ command, args }),
    });
    return reply.json();
  };
}

try {
  let invoke = await start({ executable: probe, args });
  assert.deepEqual(await invoke("get_autostart"), { ok: true, value: false });
  assert.equal((await invoke("set_autostart", { enabled: "true" })).ok, false);
  assert.deepEqual(await invoke("set_autostart", { enabled: true }), { ok: true, value: true });
  assert.deepEqual(await invoke("get_autostart"), { ok: true, value: true });
  const text = await fs.readFile(entry, "utf8");
  assert.ok(text.includes(" --tray\n"));
  const validation = spawnSync("desktop-file-validate", [entry], { encoding: "utf8" });
  if (!validation.error) {
    assert.equal(validation.status, 0, validation.stdout + validation.stderr);
    console.log("desktop-file-validate accepted the autostart entry");
  }
  const launch = spawnSync("gio", ["launch", entry], {
    env: { ...process.env, CLEVO_AUTOSTART_PROBE: probeOutput }, encoding: "utf8",
  });
  if (!launch.error) {
    assert.equal(launch.status, 0, launch.stdout + launch.stderr);
    for (let i = 0; i < 100; i++) {
      try { await fs.access(probeOutput); break; } catch { await new Promise((r) => setTimeout(r, 20)); }
    }
    assert.deepEqual((await fs.readFile(probeOutput, "utf8")).trimEnd().split("\n"), [...args, "--tray"]);
    console.log("gio launched the exact arguments, including reserved path characters");
  }
  await stop();
  invoke = await start({ executable: probe, args });
  assert.deepEqual(await invoke("get_autostart"), { ok: true, value: true });
  assert.deepEqual(await invoke("set_autostart", { enabled: false }), { ok: true, value: false });
  assert.ok((await fs.readFile(entry, "utf8")).includes("Hidden=true"));
  await stop();
  invoke = await start({ executable: "/missing/UI", args: [] });
  assert.equal((await invoke("set_autostart", { enabled: true })).ok, false);
  assert.deepEqual(await invoke("get_autostart"), { ok: true, value: false });
  assert.deepEqual(await invoke("set_autostart", { enabled: false }), { ok: true, value: false });
  console.log("Autostart bridge, persistence, disable and failed enable checks passed");
} finally {
  await stop();
  await fs.rm(dir, { recursive: true, force: true });
}
