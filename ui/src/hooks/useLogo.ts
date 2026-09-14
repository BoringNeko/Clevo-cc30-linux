import { useCallback, useEffect, useState } from "react";

/**
 * The sidebar logo image.
 *
 * Precedence: a custom logo saved by the user, otherwise the default image at
 * `defaultPath`, otherwise no image (the caller falls back to the built-in
 * icon).
 *
 * An app-relative `defaultPath` (not starting with "/", served from `public/`)
 * is used directly. An absolute filesystem path is read by the backend as a
 * data URL.
 */
export function useLogo(defaultPath: string) {
  const [logo, setLogo] = useState<string | null>(null);
  const [isCustom, setIsCustom] = useState(false);

  const loadDefault = useCallback(async () => {
    if (!defaultPath) {
      setLogo(null);
      return;
    }
    if (!defaultPath.startsWith("/")) {
      setLogo(defaultPath);
      return;
    }
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const url = await invoke<string | null>("read_image_path", { path: defaultPath });
      setLogo(url ?? null);
    } catch {
      setLogo(null);
    }
  }, [defaultPath]);

  const restore = useCallback(async () => {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const saved = await invoke<string | null>("load_logo");
      if (saved) {
        setLogo(saved);
        setIsCustom(true);
        return;
      }
    } catch {
      // not in Tauri
    }
    setIsCustom(false);
    await loadDefault();
  }, [loadDefault]);

  useEffect(() => {
    void restore();
  }, [restore]);

  const setLogoFromFile = useCallback(async (file: File) => {
    if (!file.type.startsWith("image/")) return;
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const reader = new FileReader();
      const dataUrl: string = await new Promise((resolve, reject) => {
        reader.onload = () => resolve(String(reader.result));
        reader.onerror = () => reject(reader.error);
        reader.readAsDataURL(file);
      });
      const comma = dataUrl.indexOf(",");
      const data_base64 = comma >= 0 ? dataUrl.slice(comma + 1) : dataUrl;
      const ext = file.name.includes(".") ? file.name.split(".").pop()!.toLowerCase() : "png";
      const saved = await invoke<{ data_url: string }>("save_logo", { dataBase64: data_base64, ext });
      setLogo(saved.data_url);
      setIsCustom(true);
    } catch {
      // ignore
    }
  }, []);

  const resetLogo = useCallback(async () => {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("clear_logo");
    } catch {
      // ignore
    }
    setIsCustom(false);
    await loadDefault();
  }, [loadDefault]);

  return { logo, isCustom, setLogoFromFile, resetLogo };
}
