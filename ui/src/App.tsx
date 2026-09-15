import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Alert from "@mui/material/Alert";
import Box from "@mui/material/Box";
import CssBaseline from "@mui/material/CssBaseline";
import Typography from "@mui/material/Typography";
import { ThemeProvider } from "@mui/material/styles";
import {
  getFanCurve,
  getFanSnapshot,
  pollFan,
  type FanCurve,
  type FanSnapshot,
} from "./api/daemon";
import { Sidebar } from "./components/Sidebar";
import { SettingsDialog } from "./components/SettingsDialog";
import { WindowControls } from "./components/WindowControls";
import { FansCard } from "./components/FansCard";
import { PerformanceCard } from "./components/PerformanceCard";
import { CurveCard } from "./components/CurveCard";
import { TelemetryCard } from "./components/TelemetryCard";
import { useWallpaper } from "./hooks/useWallpaper";
import { useLogo } from "./hooks/useLogo";
import { useWindowSize } from "./hooks/useWindowSize";
import {
  loadCompatibilityPrefs,
  readCompatibilityPrefs,
  useAppearance,
  useBlurPreference,
  writeCompatibilityPrefs,
  type CompatibilityPrefs,
} from "./hooks/useAppSettings";
import { cssTriplet, withAccent } from "./lib/color";
import { buildTheme, glassSx } from "./theme";

const POLL_INTERVAL_MS = 2000;
const HISTORY_LEN = 60;

/**
 * Glass dashboard over the daemon.
 *
 * Polls `clevod` for fan status and reads the curve once; the accent colour is
 * derived from the wallpaper. Writes are handled by the Performance card and go
 * through the daemon's PolicyKit gate.
 */
export default function App() {
  const { wallpaper, palette, isCustom, setWallpaperFromFile, resetWallpaper } = useWallpaper();
  const { blurEnabled, setting: blurSetting, setSetting: setBlurSetting } = useBlurPreference();
  const { appearance, update: updateAppearance } = useAppearance();
  const { logo, isCustom: logoIsCustom, setLogoFromFile, resetLogo } = useLogo(appearance.logoPath);
  const [compat, setCompat] = useState<CompatibilityPrefs>(readCompatibilityPrefs);

  // Reflect the persisted launch preferences once the backend answers.
  useEffect(() => {
    let cancelled = false;
    loadCompatibilityPrefs().then((prefs) => {
      if (!cancelled) setCompat(prefs);
    });
    return () => {
      cancelled = true;
    };
  }, []);
  // The wallpaper palette with the user's accent override applied, so every
  // consumer (gauge, curves, charts, sidebar, segmented buttons) follows it —
  // not just the MUI theme.
  const accentPalette = useMemo(
    () => withAccent(palette, appearance.accent),
    [palette, appearance.accent],
  );
  const theme = useMemo(
    () => buildTheme(accentPalette, blurEnabled, appearance),
    [accentPalette, blurEnabled, appearance],
  );

  // Keep the accent CSS variables in step with the override (the wallpaper hook
  // sets them from the raw palette; this restores the user's choice afterwards).
  useEffect(() => {
    const root = document.documentElement;
    root.style.setProperty("--accent", cssTriplet(accentPalette.primary));
    root.style.setProperty("--accent-2", cssTriplet(accentPalette.secondary));
  }, [accentPalette]);

  // Apply the UI zoom factor to the whole interface. `zoom` (supported by
  // WebKitGTK) scales fixed px sizes and MUI spacing too, unlike a root
  // font-size change which would only affect rem-based text. `scale` is a
  // percentage (100 = 1x).
  useEffect(() => {
    document.documentElement.style.zoom = appearance.scale === 100 ? "" : String(appearance.scale / 100);
  }, [appearance.scale]);

  // Keep the window size in step with the persisted display settings (applied
  // on startup and whenever the resolution changes).
  const resizeWindow = useWindowSize(appearance);

  const updateCompat = useCallback((next: CompatibilityPrefs) => {
    setCompat(next);
    writeCompatibilityPrefs(next);
  }, []);
  const [settingsOpen, setSettingsOpen] = useState(false);

  const [snapshot, setSnapshot] = useState<FanSnapshot | null>(null);
  const [curve, setCurve] = useState<FanCurve | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [active, setActive] = useState<string>("overview");
  const [cpuHistory, setCpuHistory] = useState<number[]>([]);
  const [gpuHistory, setGpuHistory] = useState<number[]>([]);
  const timer = useRef<number | null>(null);

  const applySnapshot = useCallback((snap: FanSnapshot) => {
    setSnapshot(snap);
    setCpuHistory((prev) => [...prev, snap.cpu.rpm].slice(-HISTORY_LEN));
    setGpuHistory((prev) => [...prev, snap.gpu1.rpm].slice(-HISTORY_LEN));
  }, []);

  const refresh = useCallback(async () => {
    try {
      applySnapshot(await pollFan());
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, [applySnapshot]);

  // Initial load.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [snap, cv] = await Promise.all([getFanSnapshot(), getFanCurve()]);
        if (cancelled) return;
        applySnapshot(snap);
        setCurve(cv);
        setError(null);
        await pollFan()
          .then((fresh) => !cancelled && applySnapshot(fresh))
          .catch(() => {});
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [applySnapshot]);

  // Periodic refresh.
  useEffect(() => {
    timer.current = window.setInterval(refresh, POLL_INTERVAL_MS);
    return () => {
      if (timer.current !== null) {
        window.clearInterval(timer.current);
        timer.current = null;
      }
    };
  }, [refresh]);

  // Sync the sidebar highlight with the section in view.
  useEffect(() => {
    const ids = ["overview", "fans", "performance", "curve"];
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) setActive(entry.target.id);
        }
      },
      { rootMargin: "-40% 0px -55% 0px" },
    );
    for (const id of ids) {
      const el = document.getElementById(id);
      if (el) observer.observe(el);
    }
    return () => observer.disconnect();
  }, []);

  const navigate = (id: string) => {
    setActive(id);
    document.getElementById(id)?.scrollIntoView({ behavior: "smooth", block: "start" });
  };

  return (
    <ThemeProvider theme={theme}>
      <CssBaseline />
      <Box
        sx={{
          position: "relative",
          height: "100vh",
          width: "100vw",
          overflow: "hidden",
          bgcolor: appearance.mode === "dark" ? "#000" : "#e9ebee",
        }}
      >
        <Box
          sx={{
            position: "absolute",
            inset: 0,
            backgroundImage: `url(${wallpaper})`,
            backgroundSize: "cover",
            backgroundPosition: "center",
            transition: "opacity 700ms ease",
          }}
        />
        <Box
          sx={{
            position: "absolute",
            inset: 0,
            background:
              appearance.mode === "dark"
                ? "linear-gradient(135deg, rgba(0,0,0,0.40), rgba(0,0,0,0.20), rgba(0,0,0,0.50))"
                : "linear-gradient(135deg, rgba(255,255,255,0.30), rgba(255,255,255,0.10), rgba(255,255,255,0.40))",
          }}
        />

        <Box sx={{ position: "relative", zIndex: 10, display: "flex", height: "100%", width: "100%", gap: 2, p: 2 }}>
          <Sidebar
            palette={accentPalette}
            active={active}
            blur={blurEnabled}
            appearance={appearance}
            logo={logo}
            onNavigate={navigate}
            onOpenSettings={() => setSettingsOpen(true)}
          />

          <Box
            component="main"
            sx={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 2, overflowY: "auto", pr: 0.5 }}
          >
            <Box
              component="header"
              id="overview"
              data-tauri-drag-region
              sx={{
                ...glassSx(blurEnabled, appearance),
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                px: 2.5,
                py: 1.5,
              }}
            >
              <Typography
                data-tauri-drag-region
                sx={{ fontSize: "1rem", fontWeight: 600, color: "text.primary" }}
              >
                系统概览
              </Typography>
              <WindowControls />
            </Box>

            {error && (
              <Alert severity="error" role="alert" data-testid="error">
                {error}
              </Alert>
            )}

            {snapshot ? (
              <Box sx={{ display: "grid", gridTemplateColumns: { xs: "1fr", lg: "1fr 1fr" }, gap: 2 }}>
                <Box id="fans" sx={{ scrollMarginTop: 16 }}>
                  <FansCard palette={accentPalette} snapshot={snapshot} />
                </Box>
                <Box id="performance" sx={{ scrollMarginTop: 16 }}>
                  <PerformanceCard
                    palette={accentPalette}
                    snapshot={snapshot}
                    onRefresh={refresh}
                    onError={setError}
                  />
                </Box>
                <Box id="curve" sx={{ scrollMarginTop: 16 }}>
                  {curve ? (
                    <CurveCard palette={accentPalette} curve={curve} />
                  ) : (
                    <Typography sx={{ color: "text.disabled", fontSize: "0.75rem" }}>
                      风扇曲线不可用
                    </Typography>
                  )}
                </Box>
                <TelemetryCard
                  palette={accentPalette}
                  cpuHistory={cpuHistory}
                  gpuHistory={gpuHistory}
                />
              </Box>
            ) : (
              <Typography data-testid="loading" sx={{ color: "text.disabled" }}>
                加载中…
              </Typography>
            )}
          </Box>
        </Box>

        <SettingsDialog
          open={settingsOpen}
          onClose={() => setSettingsOpen(false)}
          palette={accentPalette}
          onWallpaperChange={setWallpaperFromFile}
          onResetWallpaper={resetWallpaper}
          wallpaperIsCustom={isCustom}
          blurSetting={blurSetting}
          onBlurSettingChange={setBlurSetting}
          appearance={appearance}
          onAppearanceChange={updateAppearance}
          logo={logo}
          logoIsCustom={logoIsCustom}
          onLogoChange={setLogoFromFile}
          onLogoReset={resetLogo}
          compatibility={compat}
          onCompatibilityChange={updateCompat}
          onResize={resizeWindow}
        />
      </Box>
    </ThemeProvider>
  );
}
