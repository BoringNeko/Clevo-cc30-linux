import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Alert from "@mui/material/Alert";
import Box from "@mui/material/Box";
import CssBaseline from "@mui/material/CssBaseline";
import IconButton from "@mui/material/IconButton";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";
import { ThemeProvider } from "@mui/material/styles";
import PauseIcon from "@mui/icons-material/Pause";
import PlayArrowIcon from "@mui/icons-material/PlayArrow";
import {
  fanModeName,
  getFanCurve,
  getFanSnapshot,
  perfModeName,
  pollFan,
  type FanCurve,
  type FanSnapshot,
} from "./api/daemon";
import { Sidebar } from "./components/Sidebar";
import { SettingsDialog } from "./components/SettingsDialog";
import { FansCard } from "./components/FansCard";
import { PerformanceCard } from "./components/PerformanceCard";
import { CurveCard } from "./components/CurveCard";
import { TelemetryCard } from "./components/TelemetryCard";
import { useWallpaper } from "./hooks/useWallpaper";
import { useLogo } from "./hooks/useLogo";
import {
  loadCompatibilityPrefs,
  readCompatibilityPrefs,
  useAppearance,
  useBlurPreference,
  writeCompatibilityPrefs,
  type CompatibilityPrefs,
} from "./hooks/useAppSettings";
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
  const theme = useMemo(
    () => buildTheme(palette, blurEnabled, appearance),
    [palette, blurEnabled, appearance],
  );

  const updateCompat = useCallback((next: CompatibilityPrefs) => {
    setCompat(next);
    writeCompatibilityPrefs(next);
  }, []);
  const [settingsOpen, setSettingsOpen] = useState(false);

  const [snapshot, setSnapshot] = useState<FanSnapshot | null>(null);
  const [curve, setCurve] = useState<FanCurve | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [live, setLive] = useState(true);
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

  // Periodic refresh while live.
  useEffect(() => {
    if (!live) return;
    timer.current = window.setInterval(refresh, POLL_INTERVAL_MS);
    return () => {
      if (timer.current !== null) {
        window.clearInterval(timer.current);
        timer.current = null;
      }
    };
  }, [live, refresh]);

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
            palette={palette}
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
              sx={{
                ...glassSx(blurEnabled, appearance),
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                px: 2.5,
                py: 1.5,
              }}
            >
              <Box>
                <Typography sx={{ fontSize: "1rem", fontWeight: 600, color: "text.primary" }}>
                  系统概览
                </Typography>
                <Typography sx={{ fontSize: "0.6875rem", color: "text.disabled" }}>
                  {snapshot
                    ? `风扇模式 ${fanModeName(snapshot.fan_mode)} · 性能模式 ${perfModeName(snapshot.perf_mode)}`
                    : "正在连接 clevod…"}
                </Typography>
              </Box>
              <Tooltip title={live ? "暂停轮询" : "继续轮询"}>
                <IconButton
                  color="inherit"
                  onClick={() => setLive((v) => !v)}
                  data-testid="toggle-live"
                  aria-label={live ? "暂停" : "继续"}
                  sx={{
                    width: 32,
                    height: 32,
                    borderRadius: 1,
                    border: "1px solid divider",
                    backgroundColor: "divider",
                    color: "text.secondary",
                    "&:hover": { backgroundColor: "divider", color: "text.primary" },
                  }}
                >
                  {live ? <PauseIcon sx={{ fontSize: 16 }} /> : <PlayArrowIcon sx={{ fontSize: 16 }} />}
                </IconButton>
              </Tooltip>
            </Box>

            {error && (
              <Alert severity="error" role="alert" data-testid="error">
                {error}
              </Alert>
            )}

            {snapshot ? (
              <Box sx={{ display: "grid", gridTemplateColumns: { xs: "1fr", lg: "1fr 1fr" }, gap: 2 }}>
                <Box id="fans" sx={{ scrollMarginTop: 16 }}>
                  <FansCard palette={palette} snapshot={snapshot} />
                </Box>
                <Box id="performance" sx={{ scrollMarginTop: 16 }}>
                  <PerformanceCard
                    palette={palette}
                    snapshot={snapshot}
                    onRefresh={refresh}
                    onError={setError}
                  />
                </Box>
                <Box id="curve" sx={{ scrollMarginTop: 16 }}>
                  {curve ? (
                    <CurveCard palette={palette} curve={curve} />
                  ) : (
                    <Typography sx={{ color: "text.disabled", fontSize: "0.75rem" }}>
                      风扇曲线不可用
                    </Typography>
                  )}
                </Box>
                <TelemetryCard
                  palette={palette}
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
          palette={palette}
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
        />
      </Box>
    </ThemeProvider>
  );
}
