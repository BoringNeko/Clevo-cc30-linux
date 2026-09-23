import { useCallback, useEffect } from "react";
import { windowSize, type Appearance } from "../theme";
import { windowBridge } from "../api/bridge";

/** Resize the window to an inner logical size (no-op outside a shell). */
export async function resizeWindow(width: number, height: number): Promise<void> {
  const bridge = await windowBridge();
  if (bridge) await bridge.setSize(width, height);
}

/**
 * Keep the window size in step with the persisted display settings.
 *
 * Applies the stored size once on startup (the window otherwise opens at the
 * size from `tauri.conf.json`, so the choice would not survive a restart) and
 * again whenever the resolution changes.
 */
export function useWindowSize(appearance: Appearance) {
  const apply = useCallback(
    (width: number, height: number) => void resizeWindow(width, height),
    [],
  );

  useEffect(() => {
    const { width, height } = windowSize(appearance);
    void resizeWindow(width, height);
    // Re-run only when the chosen resolution changes, not on every appearance edit.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [appearance.displayWidth, appearance.displayHeight]);

  return apply;
}
