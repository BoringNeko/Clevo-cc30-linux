import { useEffect, useState } from "react";
import Alert from "@mui/material/Alert";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import IconButton from "@mui/material/IconButton";
import Switch from "@mui/material/Switch";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";
import RefreshIcon from "@mui/icons-material/Refresh";
import { getAutostart, setAutostart } from "../api/autostart";
import { quitApp } from "../api/daemon";
import { SettingsRow } from "./SettingsRow";

interface ApplicationSectionProps {
  /** Called after the quit request is issued (so the dialog can close). */
  onQuit?: () => void;
}

/**
 * Application-level actions.
 *
 * Closing the window only hides the app so the tray's fan and performance
 * controls keep working; this is the explicit way out. Quitting is refused
 * silently outside Tauri (plain-browser development), where the command is not
 * available.
 */
export function ApplicationSection({ onQuit }: ApplicationSectionProps) {
  const [autostart, setAutostartState] = useState<boolean | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);

  useEffect(() => {
    let active = true;
    setPending(true);
    setError(null);
    void getAutostart()
      .then((enabled) => {
        if (active) setAutostartState(enabled);
      })
      .catch((cause: unknown) => {
        if (active) setError(`无法读取自启动设置：${String(cause)}`);
      })
      .finally(() => {
        if (active) setPending(false);
      });
    return () => { active = false; };
  }, [reload]);

  async function toggleAutostart(enabled: boolean) {
    setPending(true);
    setError(null);
    try {
      setAutostartState(await setAutostart(enabled));
    } catch (cause: unknown) {
      setError(`无法保存自启动设置：${String(cause)}`);
    } finally {
      setPending(false);
    }
  }

  return (
    <Box>
      <SettingsRow title="登录时自动启动" description="仅驻留系统托盘">
        <Switch
          checked={autostart === true}
          disabled={autostart === null || pending}
          onChange={(_, enabled) => { void toggleAutostart(enabled); }}
          slotProps={{ input: { "aria-label": "登录时自动启动" } }}
        />
      </SettingsRow>
      {error && (
        <Alert
          severity="error"
          sx={{ my: 1, fontSize: "0.75rem", overflowWrap: "anywhere" }}
          action={
            <Tooltip title="重新读取">
              <IconButton
                aria-label="重新读取自启动设置"
                size="small"
                disabled={pending}
                onClick={() => setReload((value) => value + 1)}
              >
                <RefreshIcon fontSize="small" />
              </IconButton>
            </Tooltip>
          }
        >
          {error}
        </Alert>
      )}
      <SettingsRow
        title="关闭窗口的行为"
        description="点击标题栏的关闭按钮会把窗口隐藏到系统托盘，应用继续在后台运行，托盘菜单的功耗与风扇控制保持可用。要完全退出，请使用下方的按钮或托盘的“退出”。"
      >
        <Typography sx={{ fontSize: "0.75rem", color: "text.secondary" }}>
          隐藏到托盘
        </Typography>
      </SettingsRow>

      <SettingsRow
        title="退出应用程序"
        description="结束控制中心进程并移除托盘图标。风扇与性能模式由固件保持，不受影响。"
      >
        <Button
          variant="outlined"
          color="error"
          onClick={() => {
            onQuit?.();
            void quitApp().catch(() => {
              /* not running under Tauri */
            });
          }}
          sx={{ fontSize: "0.75rem" }}
        >
          退出
        </Button>
      </SettingsRow>
    </Box>
  );
}
