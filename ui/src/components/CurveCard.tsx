import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Typography from "@mui/material/Typography";
import ShowChartIcon from "@mui/icons-material/ShowChart";
import RestoreIcon from "@mui/icons-material/Restore";
import { CardHeader, GlassCard } from "./GlassCard";
import type { CurvePoint, FanCurve } from "../api/daemon";
import { setFanCurve } from "../api/daemon";
import { rgbString, type ExtractedPalette } from "../lib/color";

interface CurveCardProps {
  palette: ExtractedPalette;
  curve: FanCurve;
  /** Whether a curve can be written on this backend. */
  writable?: boolean;
  /** Called after a successful write so the parent can refresh. */
  onApplied?: () => void;
}

const W = 320;
const H = 150;
const PAD = 18;
/** Pointer distance (in viewBox units) that counts as grabbing a point. */
const HIT_RADIUS = 14;
const MIN_GAP_C = 2;

/** Map a (temp, duty) curve to the SVG polyline the design uses. */
function toPath(points: CurvePoint[]): string {
  if (points.length === 0) return "";
  return points
    .map((p, i) => {
      const x = PAD + (p.temp / 100) * (W - 2 * PAD);
      const y = H - PAD - (p.duty_pct / 100) * (H - 2 * PAD);
      return `${i === 0 ? "M" : "L"}${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
}

const clamp = (value: number, lo: number, hi: number) =>
  Math.min(hi, Math.max(lo, value));

/**
 * Move point `index` of `points`, keeping temperatures strictly increasing.
 *
 * D3-style clamping is applied to the *neighbour* points so a point can be
 * pushed all the way to its neighbour without the curve ever becoming
 * non-monotonic — which the firmware would reject.
 */
export function movePoint(
  points: CurvePoint[],
  index: number,
  temp: number,
  duty: number,
): CurvePoint[] {
  const next = points.map((p) => ({ ...p }));
  const lo = index === 0 ? 0 : points[index - 1].temp + MIN_GAP_C;
  const hi =
    index === points.length - 1 ? 100 : points[index + 1].temp - MIN_GAP_C;
  next[index].temp = clamp(Math.round(temp), lo, hi);
  next[index].duty_pct = clamp(Math.round(duty), 0, 100);
  return next;
}

/**
 * The fan curve as two restrained polylines (CPU and GPU1).
 *
 * When `writable`, the CPU points can be dragged. Editing is local until
 * "应用" is pressed, so the user can shape the whole curve and only then send
 * it to the EC (a write costs a PolicyKit prompt).
 */
export function CurveCard({ palette, curve, writable = false, onApplied }: CurveCardProps) {
  const cpuColor = rgbString(palette.primary);
  const gpuColor = rgbString(palette.secondary);

  const [draft, setDraft] = useState<CurvePoint[]>(curve.cpu);
  const [dragging, setDragging] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const svgRef = useRef<SVGSVGElement | null>(null);

  // Adopt the daemon's curve whenever it changes and the user is not editing.
  useEffect(() => {
    if (dragging === null && !busy) {
      setDraft(curve.cpu);
    }
  }, [curve.cpu, dragging, busy]);

  const dirty = useMemo(
    () =>
      draft.some(
        (p, i) => p.temp !== curve.cpu[i]?.temp || p.duty_pct !== curve.cpu[i]?.duty_pct,
      ),
    [draft, curve.cpu],
  );

  /** Convert a pointer event into (temp, duty) in viewBox units. */
  const toCurveCoords = useCallback((event: React.PointerEvent<SVGSVGElement>) => {
    const svg = svgRef.current;
    if (!svg) return null;
    const rect = svg.getBoundingClientRect();
    const x = ((event.clientX - rect.left) / rect.width) * W;
    const y = ((event.clientY - rect.top) / rect.height) * H;
    return {
      temp: ((x - PAD) / (W - 2 * PAD)) * 100,
      duty: ((H - PAD - y) / (H - 2 * PAD)) * 100,
    };
  }, []);

  const nearestPoint = useCallback(
    (event: React.PointerEvent<SVGSVGElement>) => {
      const coords = toCurveCoords(event);
      if (!coords) return null;
      let best: number | null = null;
      let bestDist = HIT_RADIUS;
      draft.forEach((p, i) => {
        const dx = ((p.temp - coords.temp) / 100) * (W - 2 * PAD);
        const dy = ((p.duty_pct - coords.duty) / 100) * (H - 2 * PAD);
        const dist = Math.hypot(dx, dy);
        if (dist <= bestDist) {
          bestDist = dist;
          best = i;
        }
      });
      return best;
    },
    [draft, toCurveCoords],
  );

  const onPointerDown = (event: React.PointerEvent<SVGSVGElement>) => {
    if (!writable) return;
    const index = nearestPoint(event);
    if (index === null) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    setDragging(index);
    setError(null);
    setNotice(null);
  };

  const onPointerMove = (event: React.PointerEvent<SVGSVGElement>) => {
    if (dragging === null) return;
    const coords = toCurveCoords(event);
    if (!coords) return;
    setDraft((current) => movePoint(current, dragging, coords.temp, coords.duty));
  };

  const endDrag = (event: React.PointerEvent<SVGSVGElement>) => {
    if (dragging === null) return;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    setDragging(null);
  };

  /**
   * Build the curve to send: CPU uses the draft, the other fans keep the
   * EC's current values so a CPU edit never silently rewrites the GPU curve.
   */
  const pendingCurve = (): FanCurve => ({ ...curve, cpu: draft });

  const apply = async () => {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await setFanCurve(pendingCurve());
      setNotice("已写入自定义曲线并切换到 customize 模式");
      onApplied?.();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const series = [
    { label: "CPU", points: draft, color: cpuColor, editable: writable },
    { label: "GPU1", points: curve.gpu1, color: gpuColor, editable: false },
  ];

  return (
    <GlassCard sx={{ gap: 2 }}>
      <CardHeader icon={<ShowChartIcon sx={{ fontSize: 16 }} />} title="风扇曲线" />

      <Box
        sx={{
          flex: 1,
          borderRadius: 1,
          border: "1px solid", borderColor: "divider",
          backgroundColor: "action.hover",
          px: 0.5,
          py: 0.5,
        }}
      >
        <Box
          component="svg"
          ref={svgRef}
          viewBox={`0 0 ${W} ${H}`}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={endDrag}
          onPointerCancel={endDrag}
          sx={{
            width: "100%",
            height: "100%",
            color: "text.primary",
            touchAction: "none",
            cursor: writable ? (dragging === null ? "crosshair" : "grabbing") : "default",
          }}
        >
          <line x1={PAD} y1={H - PAD} x2={W - PAD} y2={H - PAD} stroke="currentColor" strokeOpacity={0.18} />
          <line x1={PAD} y1={PAD} x2={PAD} y2={H - PAD} stroke="currentColor" strokeOpacity={0.18} />
          {series.map((s) => (
            <g key={s.label}>
              <path
                d={toPath(s.points)}
                fill="none"
                stroke={s.color}
                strokeWidth={1.75}
                strokeLinecap="round"
                strokeLinejoin="round"
                style={{ transition: dragging === null ? "stroke 500ms ease" : undefined }}
              />
              {s.points.map((p, i) => {
                const x = PAD + (p.temp / 100) * (W - 2 * PAD);
                const y = H - PAD - (p.duty_pct / 100) * (H - 2 * PAD);
                const active = dragging === i && s.editable;
                return (
                  <circle
                    key={`${s.label}-${i}`}
                    cx={x}
                    cy={y}
                    r={active ? 4.5 : 2.5}
                    fill={active ? "white" : s.color}
                    stroke={s.color}
                    strokeWidth={active ? 2 : 0}
                  />
                );
              })}
            </g>
          ))}
        </Box>
      </Box>

      <Box sx={{ display: "flex", flexDirection: "column", gap: 0.5 }}>
        {series.map((s) => (
          <Typography
            key={s.label}
            sx={{ fontSize: "0.6875rem", color: "text.secondary" }}
          >
            <Box component="span" sx={{ color: s.color, fontWeight: 600 }}>
              {s.label}:
            </Box>{" "}
            {s.points.map((p) => `(${p.temp}°C,${p.duty_pct}%)`).join(" ")}
          </Typography>
        ))}
      </Box>

      {writable && (
        <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
          <Button
            size="small"
            variant="outlined"
            disabled={!dirty || busy}
            onClick={apply}
            sx={{ textTransform: "none" }}
          >
            {busy ? "写入中…" : "应用曲线"}
          </Button>
          <Button
            size="small"
            startIcon={<RestoreIcon sx={{ fontSize: 14 }} />}
            disabled={!dirty || busy}
            onClick={() => {
              setDraft(curve.cpu);
              setError(null);
              setNotice(null);
            }}
            sx={{ textTransform: "none", color: "text.secondary" }}
          >
            重置
          </Button>
          <Typography sx={{ fontSize: "0.6875rem", color: "text.disabled" }}>
            拖动 CPU 折线端点编辑
          </Typography>
        </Box>
      )}

      {error && (
        <Typography sx={{ fontSize: "0.6875rem", color: "error.main" }}>
          {error}
        </Typography>
      )}
      {notice && (
        <Typography sx={{ fontSize: "0.6875rem", color: "success.main" }}>
          {notice}
        </Typography>
      )}
    </GlassCard>
  );
}
