// Typed adapter over the Tauri commands exposed by `src-tauri`.
//
// The UI never talks to hardware or D-Bus directly: every call goes through a
// Tauri command, which in turn speaks `org.clevo.CC` to `clevod`.

import { invoke } from "@tauri-apps/api/core";

/** Freshness of a cached reading, mirrored from the daemon. */
export type Freshness = "fresh" | "stale" | "unknown";

/** One fan channel as reported by the daemon. */
export interface FanReading {
  /** Speed in rpm. */
  rpm: number;
  /** Temperature in °C, or `null` when the EC reports none. */
  temp_c: number | null;
  /** Whether the channel exists on this machine. */
  available: boolean;
}

/** A four-point curve. */
export interface CurvePoint {
  temp: number;
  duty_pct: number;
}

/** The daemon's cached fan state. */
export interface FanSnapshot {
  cpu: FanReading;
  gpu1: FanReading;
  gpu2: FanReading;
  freshness: Freshness;
  fan_count: number;
  fan_mode: number;
  perf_mode: number;
  writable: boolean;
  /** Whether a custom fan curve can be written. */
  curve_writable: boolean;
}

/** Parsed fan curve. */
export interface FanCurve {
  fan_count: number;
  init_mode: number;
  kb_type: number;
  cpu: CurvePoint[];
  gpu1: CurvePoint[];
  gpu2: CurvePoint[];
}

/** Canonical names for `121/1` values; `255` means "unset". */
export const FAN_MODE_NAMES: Record<number, string> = {
  0: "auto",
  1: "max",
  5: "maxq",
  6: "custom",
  8: "quiet",
};

/**
 * The fan mode whose value selects the curve stored in the EC.
 *
 * The firmware calls this value 6 and the daemon/CLI name it `custom`; the UI
 * presents it as `customize`, because it is the mode the user picks to edit and
 * apply their own curve.
 */
export const CUSTOMIZE_FAN_MODE = 6;

/** Whether a fan mode is the one that uses the custom curve. */
export function isCustomizeMode(value: number): boolean {
  return value === CUSTOMIZE_FAN_MODE;
}

/** Canonical names for `121/25` values; `255` means "unset". */
export const PERF_MODE_NAMES: Record<number, string> = {
  0: "quiet",
  1: "pwrsaving",
  2: "performance",
  3: "entertainment",
};

/**
 * Human-readable fan mode.
 *
 * `255` means the daemon has not written a mode this session; the firmware does
 * not report the current one, so it is shown as "unknown" rather than a guess.
 */
export function fanModeName(value: number): string {
  return FAN_MODE_NAMES[value] ?? (value === 255 ? "unknown (not set)" : `unknown (${value})`);
}

/** Human-readable performance mode (see {@link fanModeName} for the 255 case). */
export function perfModeName(value: number): string {
  return PERF_MODE_NAMES[value] ?? (value === 255 ? "unknown (not set)" : `unknown (${value})`);
}

/** Read the daemon's cached fan snapshot (does not poll). */
export async function getFanSnapshot(): Promise<FanSnapshot> {
  return invoke<FanSnapshot>("get_fan_snapshot");
}

/** Ask the daemon to poll the hardware once, then return the new snapshot. */
export async function pollFan(): Promise<FanSnapshot> {
  return invoke<FanSnapshot>("poll_fan");
}

/** Read the fan curve as parsed data. */
export async function getFanCurve(): Promise<FanCurve> {
  return invoke<FanCurve>("get_fan_curve");
}

/**
 * Write a custom fan curve and select the `custom` fan mode.
 *
 * The daemon validates the curve and authorizes the write through PolicyKit;
 * a denial or an unsupported request rejects with a message the UI must show.
 */
export async function setFanCurve(curve: FanCurve): Promise<void> {
  return invoke<void>("set_fan_curve", { curve });
}

/**
 * Fan modes the UI offers, in display order.
 *
 * `label` is what the button shows; `mode` is the name the daemon accepts for a
 * write. They differ only for the curve mode, which the firmware and the daemon
 * call `custom` but the UI presents as `customize`.
 */
export const FAN_MODE_CHOICES: Array<{
  value: number;
  label: string;
  mode: string;
}> = [
  { value: 0, label: "auto", mode: "auto" },
  { value: 8, label: "quiet", mode: "quiet" },
  { value: 5, label: "maxq", mode: "maxq" },
  { value: 1, label: "max", mode: "max" },
  { value: CUSTOMIZE_FAN_MODE, label: "customize", mode: "custom" },
];

/** Performance modes the UI offers, in display order. */
export const PERF_MODE_CHOICES: Array<{ value: number; label: string }> = [
  { value: 0, label: "quiet" },
  { value: 1, label: "pwrsaving" },
  { value: 2, label: "performance" },
  { value: 3, label: "entertainment" },
];

/**
 * Set the fan mode through the daemon.
 *
 * Authorization is decided by PolicyKit inside the daemon; a denial or an
 * unsupported mode rejects with a message that the UI must show.
 */
export async function setFanMode(mode: string): Promise<number> {
  return invoke<number>("set_fan_mode", { mode });
}

/** Set the performance mode through the daemon (PolicyKit-gated). */
export async function setPerfMode(mode: string): Promise<number> {
  return invoke<number>("set_perf_mode", { mode });
}

// --- App lifecycle ----------------------------------------------------------

/**
 * Hide the window, leaving the app running in the tray.
 *
 * This is what the title bar's close button does: the control center keeps the
 * tray's fan and performance controls available, so closing the window is not
 * the same as quitting.
 */
export async function hideMainWindow(): Promise<void> {
  return invoke<void>("hide_main_window");
}

/** Quit the app entirely (window and tray icon). */
export async function quitApp(): Promise<void> {
  return invoke<void>("quit_app");
}
