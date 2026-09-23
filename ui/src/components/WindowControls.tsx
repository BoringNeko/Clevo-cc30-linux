import { useEffect, useRef, useState } from "react";
import Box from "@mui/material/Box";
import IconButton from "@mui/material/IconButton";
import Tooltip from "@mui/material/Tooltip";
import CloseIcon from "@mui/icons-material/Close";
import FullscreenExitIcon from "@mui/icons-material/FullscreenExit";
import FullscreenIcon from "@mui/icons-material/Fullscreen";
import MinimizeIcon from "@mui/icons-material/Minimize";
import { hideMainWindow } from "../api/daemon";
import { windowBridge, type WindowControlsBridge } from "../api/bridge";

/** The subset of a window the controls use. */
export type WindowHandle = WindowControlsBridge;

/**
 * Load the window handle for the current shell (Tauri or Electron), or null in
 * a plain browser where neither is available.
 */
async function loadWindow(): Promise<WindowHandle | null> {
  return windowBridge();
}

/** A 32×32 icon button matching the header's icon-button style. */
function ControlButton({
  label,
  onClick,
  danger,
  children,
}: {
  label: string;
  onClick: () => void;
  danger?: boolean;
  children: React.ReactNode;
}) {
  return (
    <Tooltip title={label}>
      <IconButton
        onClick={onClick}
        aria-label={label}
        sx={{
          width: 32,
          height: 32,
          borderRadius: 1,
          border: "1px solid", borderColor: "divider",
          backgroundColor: "action.hover",
          color: "text.secondary",
          transition: "background-color 300ms ease, color 300ms ease",
          "&:hover": {
            backgroundColor: danger ? "rgba(248,113,113,0.18)" : "action.selected",
            color: danger ? "#f87171" : "text.primary",
          },
        }}
      >
        {children}
      </IconButton>
    </Tooltip>
  );
}

export function WindowControls() {
  const [fullscreen, setFullscreen] = useState(false);
  const winRef = useRef<WindowHandle | null>(null);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      const win = await loadWindow();
      if (!win || cancelled) return;
      winRef.current = win;
      setFullscreen(await win.isFullscreen());
      unlisten = await win.onResized(() => {
        void win.isFullscreen().then(setFullscreen);
      });
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  /** Run a control action through the loaded window handle (no-op in a browser). */
  const withWindow = (fn: (w: WindowHandle) => Promise<void>) => {
    const win = winRef.current;
    if (win) void fn(win);
  };

  return (
    <Box sx={{ display: "flex", alignItems: "center", gap: 0.75 }}>
      <ControlButton label="最小化" onClick={() => withWindow((w) => w.minimize())}>
        <MinimizeIcon sx={{ fontSize: 16 }} />
      </ControlButton>
      <ControlButton
        label={fullscreen ? "退出全屏" : "全屏"}
        onClick={() => withWindow((w) => w.setFullscreen(!fullscreen))}
      >
        {fullscreen ? <FullscreenExitIcon sx={{ fontSize: 16 }} /> : <FullscreenIcon sx={{ fontSize: 16 }} />}
      </ControlButton>
      <ControlButton
        label="关闭到托盘"
        danger
        onClick={() => {
          // Closing the window keeps the app alive in the tray (the Rust side
          // also intercepts the window's own close request), so go through the
          // hide command rather than `Window.close()`.
          void hideMainWindow().catch(() => withWindow((w) => w.close()));
        }}
      >
        <CloseIcon sx={{ fontSize: 16 }} />
      </ControlButton>
    </Box>
  );
}
