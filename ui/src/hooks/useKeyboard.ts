import { useCallback, useEffect, useRef, useState } from "react";
import { getKeyboard, type KeyboardState } from "../api/daemon";

/**
 * Shared keyboard-backlight state.
 *
 * Both the settings section and the sidebar RGB page read the same controller,
 * so the fetch/write/busy logic lives here. `active` gates the work: nothing is
 * polled while the page that owns the hook is hidden, which keeps a closed
 * settings dialog from talking to the daemon.
 */
export function useKeyboard(active: boolean, pollMs = 0) {
  const [state, setState] = useState<KeyboardState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const timer = useRef<number | null>(null);

  const refresh = useCallback(async () => {
    try {
      setError(null);
      setState(await getKeyboard());
    } catch (err) {
      setError(String(err));
    }
  }, []);

  // Load on activation, and poll while active when a period is given.
  useEffect(() => {
    if (!active) {
      if (timer.current !== null) {
        window.clearInterval(timer.current);
        timer.current = null;
      }
      return;
    }
    void refresh();
    if (pollMs > 0) {
      timer.current = window.setInterval(() => void refresh(), pollMs);
    }
    return () => {
      if (timer.current !== null) {
        window.clearInterval(timer.current);
        timer.current = null;
      }
    };
  }, [active, pollMs, refresh]);

  /**
   * Run a write and refresh afterwards.
   *
   * A failed write surfaces the daemon's message and does not refresh, so the
   * card never shows a value the hardware rejected.
   */
  const run = useCallback(
    async (action: () => Promise<void>) => {
      setBusy(true);
      setError(null);
      try {
        await action();
        await refresh();
      } catch (err) {
        setError(String(err));
      } finally {
        setBusy(false);
      }
    },
    [refresh],
  );

  return { state, error, setError, busy, refresh, run };
}
