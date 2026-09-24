import { keyframes } from "@emotion/react";

/** Shared motion timing for page and settings transitions. */
export const UI_MOTION_DURATION = 260;
export const UI_MOTION_EASING = "cubic-bezier(0.22, 1, 0.36, 1)";
export const MOTION_SPEED_MIN = 50;
export const MOTION_SPEED_MAX = 200;
export const DEFAULT_MOTION_SPEED = 100;

export function motionDuration(
  speed: number = DEFAULT_MOTION_SPEED,
  enabled = true,
): number {
  if (!enabled) return 0;
  const clamped = Math.min(MOTION_SPEED_MAX, Math.max(MOTION_SPEED_MIN, speed));
  return Math.round((UI_MOTION_DURATION * 100) / clamped);
}

/** A small transform-only entrance that keeps glass blur fully rendered. */
export const UI_ENTER_KEYFRAMES = keyframes`
  from {
    transform: translate3d(0, 10px, 0);
  }
  to {
    transform: translate3d(0, 0, 0);
  }
`;

export function enterAnimation(
  speed: number = DEFAULT_MOTION_SPEED,
  enabled = true,
): string {
  if (!enabled) return "none";
  return `${UI_ENTER_KEYFRAMES} ${motionDuration(speed, enabled)}ms ${UI_MOTION_EASING} both`;
}
