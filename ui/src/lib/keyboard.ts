// Shared helpers for the keyboard-backlight UI.
//
// Both the settings section and the sidebar RGB page render the same controller
// state, so the colour/label logic lives here rather than being copied into each
// component.

import type { KeyboardMode, KeyboardState, KeyboardZone } from "../api/daemon";

/** The 6x20 per-key colour grid the daemon reports. */
export type KeyboardKeys = number[][][];

/** Lighting-mode display names (the wire spellings come from the daemon). */
const MODE_LABELS: Record<KeyboardMode, string> = {
  off: "关闭",
  static: "静态",
};

/** Short English subtitle for each mode, shown under the name. */
const MODE_SUBTITLES: Record<KeyboardMode, string> = {
  off: "Off",
  static: "Static",
};

/** Human-readable lighting mode. */
export function keyboardModeLabel(mode: KeyboardMode): string {
  return MODE_LABELS[mode] ?? mode;
}

/** English subtitle for a lighting mode. */
export function keyboardModeSubtitle(mode: KeyboardMode): string {
  return MODE_SUBTITLES[mode] ?? mode;
}

/**
 * The order lighting modes are shown in.
 *
 * The daemon may report them in any order; the UI presents one consistent
 * sequence so the grid does not move between backends or firmware revisions.
 * Anything not listed keeps its reported position after the known ones.
 */
const MODE_ORDER: KeyboardMode[] = [
  "off",
  "static",
];

/**
 * The lighting modes a controller can actually drive, in display order.
 *
 * The daemon reports the backend's own list. When it is absent (an older daemon)
 * fall back to the set every backend implements — `off`/`static` — rather than
 * assuming the wider set, so the UI never offers a control that may do nothing.
 */
export function keyboardSupportedModes(state: KeyboardState | null): KeyboardMode[] {
  const modes = state?.modes;
  const supported = modes && modes.length > 0 ? modes : (["off", "static"] as KeyboardMode[]);
  return [...supported].sort((a, b) => {
    const ai = MODE_ORDER.indexOf(a);
    const bi = MODE_ORDER.indexOf(b);
    return (ai === -1 ? MODE_ORDER.length : ai) - (bi === -1 ? MODE_ORDER.length : bi);
  });
}

/** Zone display names. */
const ZONE_LABELS: Record<KeyboardZone, string> = {
  all: "全部",
  left: "左",
  middle: "中",
  right: "右",
};

/** Human-readable zone. */
export function keyboardZoneLabel(zone: KeyboardZone): string {
  return ZONE_LABELS[zone] ?? zone;
}

/** `[r,g,b]` to a `#rrggbb` string; black when the value is missing. */
export function keysColorToHex(color: number[] | undefined): string {
  if (!color || color.length !== 3) return "#000000";
  return `#${color.map((part) => part.toString(16).padStart(2, "0")).join("")}`;
}

/**
 * A keyboard state is writable only when the backend was found *and* accepts
 * writes; the daemon reports both, and the UI must not offer controls that can
 * only fail.
 */
export function keyboardWritable(state: KeyboardState | null): boolean {
  return Boolean(state?.available && state.writable);
}

/** The 6x20 grid, padded when the daemon reports nothing usable. */
export function normaliseKeys(keys: KeyboardKeys | undefined): KeyboardKeys {
  if (keys && keys.length === 6 && keys.every((row) => row.length === 20)) return keys;
  return Array.from({ length: 6 }, () => Array.from({ length: 20 }, () => [0, 0, 0]));
}

/** Apply a colour to every key in a logical zone, returning a new grid. */
export function applyZone(
  keys: KeyboardKeys,
  zone: KeyboardZone,
  color: [number, number, number],
): KeyboardKeys {
  const columns =
    zone === "all" ? [0, 19] : zone === "left" ? [0, 6] : zone === "middle" ? [7, 12] : [13, 19];
  const next = keys.map((row) => row.map((key) => [...key]));
  for (let row = 0; row < 6; row += 1) {
    for (let col = columns[0]; col <= columns[1]; col += 1) next[row][col] = [...color];
  }
  return next;
}

/** The `acpi-dchu` backend is the single-zone RGB15 channel. */
export function isSingleZone(state: KeyboardState): boolean {
  return state.backend === "acpi-dchu";
}

/** Backend display name for the capability card. */
export function keyboardBackendLabel(state: KeyboardState): string {
  if (state.backend === "acpi-dchu") return "ACPI-DCHU RGB15 · 单区";
  if (state.backend === "usb-hid") return "USB HID · ITE 829x";
  if (state.backend === "mock") return "模拟后端";
  return "未检测到";
}

/**
 * Why the keyboard is not controllable.
 *
 * When the firmware advertises RGB15 but no Linux channel exists, the message
 * says so explicitly instead of implying the hardware is absent — the machine
 * has the lights, the driver just cannot reach them.
 */
export function keyboardUnavailableMessage(state: KeyboardState | null): string {
  if (state?.firmware_kb_type === 6 || state?.firmware_kb_type === 22) {
    return `固件报告键盘灯类型 ${state.firmware_kb_type}，但当前没有可验证的 Linux 写入通道。`;
  }
  if (state?.firmware_kb_type != null) {
    return `固件报告键盘灯类型 ${state.firmware_kb_type}，但没有可用的 Linux 写入通道。`;
  }
  return "未检测到可写的键盘灯控制器。";
}
