import { useCallback, useEffect, useState } from "react";
import {
  DEFAULT_APPEARANCE,
  RESOLUTION_PRESETS,
  SCALE_MAX,
  SCALE_MIN,
  defaultSize,
  type Appearance,
  type AspectRatio,
} from "../theme";

/**
 * Whether to use the frosted-glass blur.
 *
 * `backdrop-filter` only works while WebKit's accelerated compositor is alive.
 * The backend forces the DMA-BUF transport onto shared memory
 * (`WEBKIT_DMABUF_RENDERER_FORCE_SHM=1`) so the compositor survives the NVIDIA
 * GBM bug; that keeps acceleration and blur working. If the user instead turns
 * on software rendering, WebKit disables the compositor entirely and blur
 * becomes a no-op — the cards would then be too transparent to read.
 *
 * Rather than guess, this returns:
 *  - `false` when the CSS API is absent, or
 *  - when the user has forced blur off in Settings → Compatibility.
 *
 * A runtime setting (`clevo.blur = "on" | "off" | "auto"`) lets the user pin the
 * choice; the default `auto` uses the CSS feature detection.
 */
export type BlurSetting = "auto" | "on" | "off";

const STORAGE_KEY = "clevo.blur";

/** localStorage when available (browser/WebView, not SSR/Node). */
function storage(): Storage | null {
  try {
    return typeof window !== "undefined" ? window.localStorage : null;
  } catch {
    return null;
  }
}

function cssSupportsBlur(): boolean {
  if (typeof CSS === "undefined" || typeof CSS.supports !== "function") return false;
  return (
    CSS.supports("backdrop-filter", "blur(1px)") ||
    CSS.supports("-webkit-backdrop-filter", "blur(1px)")
  );
}

/** Read the persisted blur setting. */
export function readBlurSetting(): BlurSetting {
  const store = storage();
  const value = store ? store.getItem(STORAGE_KEY) : null;
  return value === "on" || value === "off" ? value : "auto";
}

/** Persist the blur setting. */
export function writeBlurSetting(value: BlurSetting) {
  storage()?.setItem(STORAGE_KEY, value);
}

/**
 * Whether to render the frosted blur, honouring the persisted setting.
 * Returns whether blur is enabled and the current setting, plus a setter.
 */
export function useBlurPreference(): {
  blurEnabled: boolean;
  setting: BlurSetting;
  setSetting: (value: BlurSetting) => void;
} {
  const [setting, setSettingState] = useState<BlurSetting>(readBlurSetting);
  const [cssOk] = useState<boolean>(cssSupportsBlur);

  useEffect(() => {
    writeBlurSetting(setting);
  }, [setting]);

  const blurEnabled = setting === "on" || (setting === "auto" && cssOk);
  return { blurEnabled, setting, setSetting: setSettingState };
}

/**
 * Resolve the stored app preferences for backend / software rendering. These
 * cannot change a running WebView, so they are persisted and applied on the
 * next launch by the start script.
 */
export interface CompatibilityPrefs {
  backend: "auto" | "wayland" | "x11";
  softwareRendering: boolean;
}

const BACKEND_KEY = "clevo.backend";
const SOFTWARE_KEY = "clevo.softwareRendering";

export function readCompatibilityPrefs(): CompatibilityPrefs {
  const store = storage();
  const backend = store ? store.getItem(BACKEND_KEY) : null;
  const software = store ? store.getItem(SOFTWARE_KEY) : null;
  return {
    backend: backend === "wayland" || backend === "x11" ? backend : "auto",
    softwareRendering: software === "true",
  };
}

export function writeCompatibilityPrefs(prefs: CompatibilityPrefs) {
  // Mirror to localStorage for instant reads, and persist via the backend so
  // scripts/run-ui.sh can read the same choice before the WebView starts.
  const store = storage();
  if (store) {
    store.setItem(BACKEND_KEY, prefs.backend);
    store.setItem(SOFTWARE_KEY, String(prefs.softwareRendering));
  }
  void persistLaunchPrefs(prefs);
}

/** Persist through the Tauri backend when available (imported lazily). */
async function persistLaunchPrefs(prefs: CompatibilityPrefs): Promise<void> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("set_launch_prefs", {
      prefs: { backend: prefs.backend, software_rendering: prefs.softwareRendering },
    });
  } catch {
    // Not running inside Tauri (e.g. plain browser dev); localStorage is enough.
  }
}

/**
 * Load the compatibility prefs from the backend, falling back to localStorage.
 * Call once at startup so the UI reflects what the launch script will use.
 */
export async function loadCompatibilityPrefs(): Promise<CompatibilityPrefs> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const prefs = await invoke<{ backend: string; software_rendering: boolean }>(
      "get_launch_prefs",
    );
    return {
      backend:
        prefs.backend === "wayland" || prefs.backend === "x11" ? prefs.backend : "auto",
      softwareRendering: prefs.software_rendering,
    };
  } catch {
    return readCompatibilityPrefs();
  }
}


const APPEARANCE_KEY = "clevo.appearance";

/** Read the persisted appearance (mode + colour overrides). */
export function readAppearance(): Appearance {
  const store = storage();
  const raw = store ? store.getItem(APPEARANCE_KEY) : null;
  if (!raw) return DEFAULT_APPEARANCE;
  try {
    const parsed = JSON.parse(raw) as Partial<Appearance>;
    const blurPx = typeof parsed.blurPx === "number" ? parsed.blurPx : DEFAULT_APPEARANCE.blurPx;
    const opacity = typeof parsed.opacity === "number" ? parsed.opacity : null;
    const scale = typeof parsed.scale === "number" ? parsed.scale : DEFAULT_APPEARANCE.scale;
    const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
    const aspect: AspectRatio = parsed.aspect === "16:10" ? "16:10" : "16:9";
    // Snap the stored size to a preset of the stored ratio; anything else falls
    // back to that ratio's default. Keeps the dropdown in sync with the value.
    const presets = RESOLUTION_PRESETS[aspect];
    const preset = presets.find(
      (p) => p.width === parsed.displayWidth && p.height === parsed.displayHeight,
    );
    const fallback = defaultSize(aspect);
    const [displayWidth, displayHeight] = preset
      ? [preset.width, preset.height]
      : [fallback.width, fallback.height];
    return {
      mode: parsed.mode === "light" ? "light" : "dark",
      accent: typeof parsed.accent === "string" ? parsed.accent : null,
      surface: typeof parsed.surface === "string" ? parsed.surface : null,
      textColor: typeof parsed.textColor === "string" ? parsed.textColor : null,
      opacity: opacity === null ? null : clamp(opacity, 0, 1),
      blurPx: clamp(blurPx, 0, 40),
      brandTitle: typeof parsed.brandTitle === "string" ? parsed.brandTitle : DEFAULT_APPEARANCE.brandTitle,
      brandSubtitle:
        typeof parsed.brandSubtitle === "string" ? parsed.brandSubtitle : DEFAULT_APPEARANCE.brandSubtitle,
      logoPath: typeof parsed.logoPath === "string" ? parsed.logoPath : DEFAULT_APPEARANCE.logoPath,
      aspect,
      displayWidth,
      displayHeight,
      scale: clamp(scale, SCALE_MIN, SCALE_MAX),
    };
  } catch {
    return DEFAULT_APPEARANCE;
  }
}

/** Persist the appearance. */
export function writeAppearance(appearance: Appearance) {
  storage()?.setItem(APPEARANCE_KEY, JSON.stringify(appearance));
}

/** Appearance state with persistence. */
export function useAppearance(): {
  appearance: Appearance;
  setAppearance: (value: Appearance) => void;
  update: (patch: Partial<Appearance>) => void;
} {
  const [appearance, setAppearanceState] = useState<Appearance>(readAppearance);
  useEffect(() => {
    writeAppearance(appearance);
  }, [appearance]);
  const update = useCallback((patch: Partial<Appearance>) => {
    setAppearanceState((prev) => ({ ...prev, ...patch }));
  }, []);
  return { appearance, setAppearance: setAppearanceState, update };
}
