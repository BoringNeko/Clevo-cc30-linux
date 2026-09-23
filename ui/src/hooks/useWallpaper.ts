import { useCallback, useEffect, useRef, useState } from "react";
import { invokeBridge as invoke } from "../api/bridge";
import {
  cssTriplet,
  extractPalette,
  FALLBACK_PALETTE,
  type ExtractedPalette,
} from "../lib/color";

const DEFAULT_WALLPAPER = "/wallpaper-default.svg";

/** Read a File as base64 (without the data: prefix). */
function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = String(reader.result);
      const comma = result.indexOf(",");
      resolve(comma >= 0 ? result.slice(comma + 1) : result);
    };
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
}

function extOf(file: File): string {
  const fromName = file.name.includes(".") ? file.name.split(".").pop()! : "";
  if (fromName) return fromName.toLowerCase();
  return file.type.split("/").pop() ?? "png";
}

/** Persist via the Tauri backend when available (lazy import). */
async function persistWallpaper(file: File): Promise<string | null> {
  try {
    const data_base64 = await fileToBase64(file);
    const saved = await invoke<{ data_url: string }>("save_wallpaper", {
      dataBase64: data_base64,
      ext: extOf(file),
    });
    return saved.data_url;
  } catch {
    return null;
  }
}

/**
 * Wallpaper state, persisted across restarts.
 *
 * The chosen image is copied into the app data directory by the backend and read
 * back as a data URL, so it survives a restart (a blob URL would not). Outside
 * Tauri (plain browser dev) it falls back to a blob URL for the session.
 */
export function useWallpaper() {
  const [wallpaper, setWallpaper] = useState<string>(DEFAULT_WALLPAPER);
  const [palette, setPalette] = useState<ExtractedPalette>(FALLBACK_PALETTE);
  const [isCustom, setIsCustom] = useState(false);
  const objectUrlRef = useRef<string | null>(null);

  const applyPalette = useCallback((p: ExtractedPalette) => {
    setPalette(p);
    const root = document.documentElement;
    root.style.setProperty("--accent", cssTriplet(p.primary));
    root.style.setProperty("--accent-2", cssTriplet(p.secondary));
  }, []);

  const loadPaletteFromUrl = useCallback(
    (url: string) => {
      const img = new Image();
      img.crossOrigin = "anonymous";
      img.onload = () => applyPalette(extractPalette(img));
      img.onerror = () => applyPalette(FALLBACK_PALETTE);
      img.src = url;
    },
    [applyPalette],
  );

  // Restore the saved wallpaper on first load.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const saved = await invoke<string | null>("load_wallpaper");
        if (!cancelled && saved) {
          setWallpaper(saved);
          setIsCustom(true);
          loadPaletteFromUrl(saved);
          return;
        }
      } catch {
        // Not in Tauri; keep the default.
      }
      if (!cancelled) loadPaletteFromUrl(DEFAULT_WALLPAPER);
    })();
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const setWallpaperFromFile = useCallback(
    async (file: File) => {
      if (!file.type.startsWith("image/")) return;
      const saved = await persistWallpaper(file);
      if (saved) {
        if (objectUrlRef.current) {
          URL.revokeObjectURL(objectUrlRef.current);
          objectUrlRef.current = null;
        }
        setWallpaper(saved);
        setIsCustom(true);
        loadPaletteFromUrl(saved);
        return;
      }
      // Fallback (browser dev): a session-only blob URL.
      const url = URL.createObjectURL(file);
      if (objectUrlRef.current) URL.revokeObjectURL(objectUrlRef.current);
      objectUrlRef.current = url;
      setWallpaper(url);
      setIsCustom(true);
      loadPaletteFromUrl(url);
    },
    [loadPaletteFromUrl],
  );

  const resetWallpaper = useCallback(async () => {
    try {
      await invoke("clear_wallpaper");
    } catch {
      // ignore
    }
    if (objectUrlRef.current) {
      URL.revokeObjectURL(objectUrlRef.current);
      objectUrlRef.current = null;
    }
    setWallpaper(DEFAULT_WALLPAPER);
    setIsCustom(false);
    loadPaletteFromUrl(DEFAULT_WALLPAPER);
  }, [loadPaletteFromUrl]);

  useEffect(() => {
    return () => {
      if (objectUrlRef.current) URL.revokeObjectURL(objectUrlRef.current);
    };
  }, []);

  return { wallpaper, palette, isCustom, setWallpaperFromFile, resetWallpaper };
}
