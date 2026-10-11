import { invokeBridge as invoke } from "./bridge";

export interface ChargeThresholds {
  enabled?: boolean | null;
  start: number | null;
  end: number;
}

export interface Battery {
  name: string;
  present: boolean;
  capacity: number | null;
  status: string | null;
  health: string | null;
  health_percent: number | null;
  cycle_count: number | null;
  thresholds: ChargeThresholds | null;
  charge_control?: { backend: string; start_values: number[]; end_values: number[] } | null;
  start_supported: boolean;
  end_supported: boolean;
  errors: string[];
}

export interface PowerSnapshot {
  available: boolean;
  ac_online: boolean | null;
  batteries: Battery[];
  platform_profile: {
    driver: string | null;
    current: string | null;
    choices: string[];
    reason: string | null;
  };
  errors: string[];
}

export function getPower(): Promise<PowerSnapshot> {
  return invoke<PowerSnapshot>("get_power");
}

export function setChargeThresholds(battery: string, start: number | null, end: number, enabled?: boolean): Promise<ChargeThresholds> {
  return invoke<ChargeThresholds>("set_charge_thresholds", { battery, start, end, ...(enabled !== undefined ? { enabled } : {}) });
}

/** Logical firmware values, shared with the kernel platform-profile mapping. */
export const DESKTOP_PROFILES = [
  { profile: "low-power", mode: "pwrsaving", label: "节能" },
  { profile: "quiet", mode: "quiet", label: "安静" },
  { profile: "balanced", mode: "entertainment", label: "均衡" },
  { profile: "performance", mode: "performance", label: "高性能" },
] as const;
