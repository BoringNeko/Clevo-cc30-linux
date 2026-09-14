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
  /** Raw duty byte (conversion unverified). */
  duty: number;
  /** Raw temperature byte (conversion unverified). */
  temp_raw: number;
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
  8: "quiet",
};

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

/** Fan modes the UI offers, in display order. */
export const FAN_MODE_CHOICES: Array<{ value: number; label: string }> = [
  { value: 0, label: "auto" },
  { value: 8, label: "quiet" },
  { value: 5, label: "maxq" },
  { value: 1, label: "max" },
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
