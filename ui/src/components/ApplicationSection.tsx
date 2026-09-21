import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Typography from "@mui/material/Typography";
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
  return (
    <Box>
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
