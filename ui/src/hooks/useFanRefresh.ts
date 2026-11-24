import { useCallback, useEffect, useRef } from "react";
import { isWindowVisible } from "../api/bridge";
import { pollFan, type FanSnapshot } from "../api/daemon";

const FAN_REFRESH_INTERVAL_MS = 60_000;

export function useFanRefresh(
  enabled: boolean,
  onSnapshot: (snapshot: FanSnapshot) => void,
  onError: (message: string | null) => void,
) {
  const pending = useRef<Promise<void> | null>(null);
  const nextRefreshAt = useRef(0);

  const refreshFan = useCallback(() => {
    if (pending.current) return pending.current;
    pending.current = (async () => {
      try {
        onSnapshot(await pollFan());
        onError(null);
      } catch (error) {
        onError(String(error));
      } finally {
        nextRefreshAt.current = performance.now() + FAN_REFRESH_INTERVAL_MS;
        pending.current = null;
      }
    })();
    return pending.current;
  }, [onSnapshot, onError]);

  useEffect(() => {
    if (!enabled) return;
    let disposed = false;
    let checking = false;
    let timer: number | undefined;

    const update = async () => {
      window.clearTimeout(timer);
      if (disposed || checking || document.hidden) return;

      const remaining = Math.ceil(nextRefreshAt.current - performance.now());
      if (remaining > 0) {
        timer = window.setTimeout(update, remaining);
        return;
      }

      checking = true;
      const visible = await isWindowVisible();
      if (visible && !disposed && !document.hidden && performance.now() >= nextRefreshAt.current) {
        await refreshFan();
      }
      checking = false;

      if (!disposed && !document.hidden) {
        const delay = visible
          ? Math.max(0, Math.ceil(nextRefreshAt.current - performance.now()))
          : FAN_REFRESH_INTERVAL_MS;
        timer = window.setTimeout(update, delay);
      }
    };

    document.addEventListener("visibilitychange", update);
    window.addEventListener("focus", update);
    void update();
    return () => {
      disposed = true;
      window.clearTimeout(timer);
      document.removeEventListener("visibilitychange", update);
      window.removeEventListener("focus", update);
    };
  }, [enabled, refreshFan]);
}
