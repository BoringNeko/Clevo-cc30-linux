import Alert from "@mui/material/Alert";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import DialogActions from "@mui/material/DialogActions";
import DialogContent from "@mui/material/DialogContent";
import DialogContentText from "@mui/material/DialogContentText";
import DialogTitle from "@mui/material/DialogTitle";
import Typography from "@mui/material/Typography";
import CheckIcon from "@mui/icons-material/Check";
import DeveloperBoardIcon from "@mui/icons-material/DeveloperBoard";
import ScheduleIcon from "@mui/icons-material/Schedule";
import { CardHeader, GlassCard } from "./GlassCard";
import { SlideDialog, type DialogMotion } from "./SlideDialog";
import { modeButtonSx } from "./modeButtonSx";
import type { useMux } from "../hooks/useMux";
import type { ExtractedPalette } from "../lib/color";

const MODES = [
  { value: 2, mode: "discrete", label: "独显直连" },
  { value: 3, mode: "hybrid", label: "混合模式" },
] as const;

interface MuxCardProps {
  palette: ExtractedPalette;
  mux: ReturnType<typeof useMux>;
  motion: DialogMotion;
}

export function MuxCard({ palette, mux, motion }: MuxCardProps) {
  const { status, error, submitting, restartNotice, dismissRestartNotice, reload, apply } = mux;
  const busy = submitting !== null;
  const target = submitting ?? (status?.requested_mode || status?.current_mode);
  const pending = target !== status?.current_mode
    ? MODES.find((choice) => choice.value === target)
    : undefined;
  const selectedColor = pending ? palette.secondary : palette.primary;
  const statusText = busy ? "正在提交显卡模式…"
    : status === undefined ? (error ? null : "正在读取显卡模式…")
    : status === null ? "当前设备或服务未提供显卡切换功能"
    : pending ? `待重启 · 切换为${pending.label}` : null;

  return (
    <GlassCard sx={{ gap: 2.5 }}>
      <CardHeader icon={<DeveloperBoardIcon sx={{ fontSize: 16 }} />} title="显卡模式" />
      {status && (
        <Box role="group" aria-label="显卡模式" aria-busy={busy} sx={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 1 }}>
          {MODES.map((choice) => {
            const supported = Boolean(status.supported_mask & (1 << (choice.value - 1)));
            const selected = target === choice.value;
            const blocked = selected || busy || !supported;
            const waiting = selected && Boolean(pending);
            const Icon = waiting ? ScheduleIcon : CheckIcon;
            return (
              <Button
                key={choice.mode}
                onClick={() => {
                  if (!blocked) void apply(choice);
                }}
                disabled={!supported}
                aria-disabled={blocked}
                variant="outlined"
                aria-pressed={selected}
                startIcon={selected ? <Icon sx={{ fontSize: 15 }} /> : undefined}
                sx={modeButtonSx(selectedColor, selected)}
              >
                {choice.label}
              </Button>
            );
          })}
        </Box>
      )}
      {error && (
        <Alert
          severity="error"
          sx={{ overflowWrap: "anywhere" }}
          action={status === undefined && <Button color="inherit" size="small" onClick={reload} disabled={busy}>重试</Button>}
        >
          {error}
        </Alert>
      )}
      {statusText && (
        <Typography role="status" sx={{ fontSize: "0.75rem", color: "text.secondary" }}>
          {statusText}
        </Typography>
      )}
      <SlideDialog
        open={restartNotice && Boolean(pending)}
        onClose={dismissRestartNotice}
        aria-describedby="mux-restart-description"
        maxWidth="xs"
        fullWidth
        motion={motion}
        slotProps={{
          backdrop: { sx: { backgroundColor: "rgba(0,0,0,0.55)", backdropFilter: "blur(4px)" } },
          paper: {
            sx: (theme) => ({
              backgroundColor: theme.palette.mode === "dark" ? "rgba(14,14,18,0.92)" : "rgba(250,250,252,0.95)",
              backgroundImage: "none",
              border: "1px solid",
              borderColor: "divider",
              borderRadius: 2,
            }),
          },
        }}
      >
        <DialogTitle>显卡模式待生效</DialogTitle>
        <DialogContent>
          <DialogContentText id="mux-restart-description">
            重启电脑后将切换为「{pending?.label}」。
          </DialogContentText>
        </DialogContent>
        <DialogActions sx={{ px: 3, pb: 2 }}>
          <Button autoFocus variant="outlined" onClick={dismissRestartNotice}>知道了</Button>
        </DialogActions>
      </SlideDialog>
    </GlassCard>
  );
}
