import { useCallback, useEffect, useState } from "react";
import { getMuxStatus, setMuxMode, type MuxMode, type MuxStatus } from "../api/daemon";

/** Keep the request in flight when the user navigates away from the overview. */
export function useMux() {
  const [status, setStatus] = useState<MuxStatus | null>();
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState<number | null>(null);
  const [restartNotice, setRestartNotice] = useState(false);

  const read = useCallback(async (signal?: AbortSignal) => {
    try {
      const next = await getMuxStatus();
      if (!signal?.aborted) setStatus(next);
      return next;
    } catch (e) {
      if (signal?.aborted) return;
      setStatus(undefined);
      setError(`无法读取显卡模式：${String(e)}`);
    }
  }, []);

  useEffect(() => {
    const controller = new AbortController();
    void read(controller.signal);
    return () => controller.abort();
  }, [read]);

  const reload = () => {
    setError(null);
    void read();
  };

  const apply = async ({ mode, value }: { mode: MuxMode; value: number }) => {
    setSubmitting(value);
    setError(null);
    setRestartNotice(false);
    let submitted = false;
    try {
      await setMuxMode(mode);
      submitted = true;
    } catch (e) {
      setError(`无法提交显卡模式：${String(e)}`);
    } finally {
      const next = await read();
      setSubmitting(null);
      if (submitted && next?.requested_mode === value && next.current_mode !== value) {
        setRestartNotice(true);
      }
    }
  };

  return {
    status,
    error,
    submitting,
    restartNotice,
    dismissRestartNotice: () => setRestartNotice(false),
    reload,
    apply,
  };
}
