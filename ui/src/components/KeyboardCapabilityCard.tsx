import Box from "@mui/material/Box";
import Stack from "@mui/material/Stack";
import Typography from "@mui/material/Typography";
import TuneIcon from "@mui/icons-material/Tune";
import KeyboardIcon from "@mui/icons-material/Keyboard";
import { CardHeader, GlassCard } from "./GlassCard";
import type { KeyboardState } from "../api/daemon";
import {
  keyboardBackendLabel,
  isSingleZone,
  keyboardUnavailableMessage,
  keyboardWritable,
} from "../lib/keyboard";

interface KeyboardCapabilityCardProps {
  state: KeyboardState | null;
}

/** One label/value row. */
function Row({ label, value }: { label: string; value: string }) {
  return (
    <Box
      sx={{
        display: "flex",
        alignItems: "baseline",
        justifyContent: "space-between",
        gap: 2,
        py: 1,
        borderBottom: "1px solid", borderBottomColor: "divider",
        "&:last-of-type": { borderBottom: "none" },
      }}
    >
      <Typography sx={{ fontSize: "0.75rem", color: "text.disabled" }}>{label}</Typography>
      <Typography
        sx={{
          fontSize: "0.75rem",
          fontWeight: 500,
          color: "text.primary",
          fontVariantNumeric: "tabular-nums",
          textAlign: "right",
        }}
      >
        {value}
      </Typography>
    </Box>
  );
}

/**
 * What the machine actually exposes.
 *
 * This card exists to keep the RGB page honest: it names the detected backend,
 * whether it accepts writes, the firmware's keyboard type and how many zones
 * are addressable — so a single-zone machine is never shown multi-zone
 * controls, and a machine with the lights but no Linux channel says so.
 */
export function KeyboardCapabilityCard({ state }: KeyboardCapabilityCardProps) {
  const available = Boolean(state?.available);
  const writable = keyboardWritable(state);
  const backend = state ? keyboardBackendLabel(state) : "读取中…";
  const zones = state && !isSingleZone(state) ? "全部 / 左 / 中 / 右" : "单区（整块键盘共用）";
  const firmware = state?.firmware_kb_type != null ? String(state.firmware_kb_type) : "未报告";
  const usb =
    state?.backend === "usb-hid" && state.vendor_id != null && state.product_id != null
      ? `${state.vendor_id.toString(16).padStart(4, "0")}:${state.product_id
          .toString(16)
          .padStart(4, "0")}`
      : "—";

  return (
    <GlassCard sx={{ gap: 2 }}>
      <CardHeader icon={<TuneIcon sx={{ fontSize: 16 }} />} title="控制器" hint={available ? "已检测" : "不可用"} />

      {!available ? (
        <Stack spacing={1} sx={{ py: 1 }}>
          <KeyboardIcon sx={{ color: "text.disabled", fontSize: 28 }} />
          <Typography sx={{ fontSize: "0.8125rem", fontWeight: 600 }}>
            {keyboardUnavailableMessage(state)}
          </Typography>
          <Typography sx={{ fontSize: "0.75rem", color: "text.disabled" }}>
            支持 ITE 829x USB 控制器（048d:8910）的机型走 USB HID 逐键控制；RGB15 机型需要
            内核驱动的 keyboard_rgb 节点。
          </Typography>
        </Stack>
      ) : (
        <Box>
          <Row label="后端" value={backend} />
          <Row label="可写入" value={writable ? "是" : "否"} />
          <Row label="灯区" value={zones} />
          <Row
            label="灯效"
            value={state?.modes?.length ? `${state.modes.length} 种` : "关闭 / 静态 / 波浪"}
          />
          <Row label="固件 kb_type" value={firmware} />
          <Row label="USB ID" value={usb} />
          <Row label="亮度档位" value="0 – 100（百分比）" />
        </Box>
      )}
    </GlassCard>
  );
}
