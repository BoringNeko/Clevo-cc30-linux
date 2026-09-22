import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Typography from "@mui/material/Typography";
import CheckIcon from "@mui/icons-material/Check";
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
const HIT_RADIUS = 16;
const MIN_GAP_C = 2;

/**
 * The point indices command 14 actually carries.
 *
 * The Windows stack sends only the middle two points of each fan; the EC keeps
 * its own first and last. Only these two are therefore editable - offering the
 * others would let a user move a point that is never written back.
 */
export const EDITABLE_INDICES: readonly number[] = [1, 2];

/** Whether a point of a fan curve can be moved. */
export function isEditablePoint(index: number): boolean {
  return EDITABLE_INDICES.includes(index);
}

/** The two fans the firmware writes through command 14. */
type Channel = "cpu" | "gpu1";

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

/** Whether two curves hold the same points, in order. */
export function sameCurve(a: CurvePoint[], b: CurvePoint[]): boolean {
  return (
    a.length === b.length &&
    a.every((p, i) => p.temp === b[i]?.temp && p.duty_pct === b[i]?.duty_pct)
  );
}

/**
 * Whether a curve arriving from the daemon should replace the local draft.
 *
 * Two things must both hold: the curve is genuinely new (compared by content,
 * not by object identity, because every poll re-serialises it), and the user
 * has no unsaved edit that it would discard.
 *
 * "Unsaved edit" is measured against the curve the daemon last reported
 * (`adopted`), not against the incoming one - otherwise a curve that merely
 * arrived from elsewhere would look like a local edit and be wrongly held back.
 */
export function shouldAdoptCurve(
  incoming: CurvePoint[],
  adopted: CurvePoint[],
  draft: CurvePoint[],
): boolean {
  return !sameCurve(incoming, adopted) && sameCurve(draft, adopted);
}

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

/** One draggable series: its name, colour and current points. */
interface Series {
  key: Channel;
  label: string;
  color: string;
  points: CurvePoint[];
}

/**
 * The fan curve as two polylines (CPU and GPU1), both draggable.
 *
 * Editing is local until "应用曲线" is pressed, so the user can shape both
 * curves and only then send them to the EC (a write costs a PolicyKit prompt).
 */
export function CurveCard({ palette, curve, writable = false, onApplied }: CurveCardProps) {
  const colors: Record<Channel, string> = {
    cpu: rgbString(palette.primary),
    gpu1: rgbString(palette.secondary),
  };

  const [draft, setDraft] = useState<FanCurve>(curve);
  const [dragging, setDragging] = useState<{ channel: Channel; index: number } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const svgRef = useRef<SVGSVGElement | null>(null);

  // The daemon curve this component last agreed on, per channel, compared by
  // *content*. Using `dragging` as an effect dependency once made a release
  // reset the draft; content comparison is what actually matters.
  const adopted = useRef({ cpu: curve.cpu, gpu1: curve.gpu1 });
  const draftRef = useRef(draft);
  draftRef.current = draft;

  useEffect(() => {
    const incoming = { cpu: curve.cpu, gpu1: curve.gpu1 };
    const take = (channel: Channel) =>
      shouldAdoptCurve(incoming[channel], adopted.current[channel], draftRef.current[channel]);
    if (take("cpu") || take("gpu1")) {
      setDraft((current) => ({
        ...current,
        cpu: take("cpu") ? incoming.cpu : current.cpu,
        gpu1: take("gpu1") ? incoming.gpu1 : current.gpu1,
      }));
    }
    if (take("cpu")) adopted.current.cpu = incoming.cpu;
    if (take("gpu1")) adopted.current.gpu1 = incoming.gpu1;
  }, [curve.cpu, curve.gpu1]);

  /** Channels whose draft differs from what the daemon last reported. */
  const dirtyChannels = useMemo(() => {
    const list: Channel[] = [];
    if (!sameCurve(draft.cpu, curve.cpu)) list.push("cpu");
    if (!sameCurve(draft.gpu1, curve.gpu1)) list.push("gpu1");
    return list;
  }, [draft, curve.cpu, curve.gpu1]);
  const dirty = dirtyChannels.length > 0;

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

  /**
   * Find the point under the pointer, across *both* series.
   *
   * Both curves are draggable, so a hit is a (channel, index) pair. Only the
   * points command 14 carries are considered (see {@link EDITABLE_INDICES}):
   * including the fixed first and last points let a nearby grab snap to one of
   * them, which is why the third point could not be picked up.
   *
   * The nearest wins; on a tie the later-drawn series (GPU1) wins, matching
   * what the user sees, and the drag label names which line was grabbed.
   */
  const nearestPoint = useCallback(
    (event: React.PointerEvent<SVGSVGElement>): { channel: Channel; index: number } | null => {
      const coords = toCurveCoords(event);
      if (!coords) return null;
      let best: { channel: Channel; index: number } | null = null;
      let bestDist = HIT_RADIUS;
      (["cpu", "gpu1"] as Channel[]).forEach((channel) => {
        EDITABLE_INDICES.forEach((i) => {
          const p = draft[channel][i];
          if (!p) return;
          const dx = ((p.temp - coords.temp) / 100) * (W - 2 * PAD);
          const dy = ((p.duty_pct - coords.duty) / 100) * (H - 2 * PAD);
          const dist = Math.hypot(dx, dy);
          // `<=` lets a later series take an exact tie.
          if (dist <= bestDist) {
            bestDist = dist;
            best = { channel, index: i };
          }
        });
      });
      return best;
    },
    [draft, toCurveCoords],
  );

  const onPointerDown = (event: React.PointerEvent<SVGSVGElement>) => {
    if (!writable) return;
    const hit = nearestPoint(event);
    if (hit === null) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    setDragging(hit);
    setError(null);
    setNotice(null);
  };

  const onPointerMove = (event: React.PointerEvent<SVGSVGElement>) => {
    if (dragging === null) return;
    const coords = toCurveCoords(event);
    if (!coords) return;
    const { channel, index } = dragging;
    setDraft((current) => ({
      ...current,
      [channel]: movePoint(current[channel], index, coords.temp, coords.duty),
    }));
  };

  const endDrag = (event: React.PointerEvent<SVGSVGElement>) => {
    if (dragging === null) return;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    setDragging(null);
  };

  const apply = async () => {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await setFanCurve({ ...curve, cpu: draft.cpu, gpu1: draft.gpu1 });
      setNotice(
        dirtyChannels.length === 1
          ? `已写入 ${dirtyChannels[0] === "cpu" ? "CPU" : "GPU1"} 曲线并切换到 customize 模式`
          : "已写入 CPU 与 GPU1 曲线并切换到 customize 模式",
      );
      onApplied?.();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const reset = () => {
    setDraft({ ...draft, cpu: curve.cpu, gpu1: curve.gpu1 });
    adopted.current = { cpu: curve.cpu, gpu1: curve.gpu1 };
    setError(null);
    setNotice(null);
  };

  const series: Series[] = [
    { key: "cpu", label: "CPU", color: colors.cpu, points: draft.cpu },
    { key: "gpu1", label: "GPU1", color: colors.gpu1, points: draft.gpu1 },
  ];

  return (
    <GlassCard sx={{ gap: 1.5 }}>
      <CardHeader
        icon={<ShowChartIcon sx={{ fontSize: 16 }} />}
        title="风扇曲线"
        hint={dirty ? "未应用" : undefined}
      />

      <Legend palette={palette} dirtyChannels={dirtyChannels} />

      <Box
        sx={{
          flex: 1,
          minHeight: 0,
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
          /*
           * Stretch to fill the box rather than letterboxing it.
           *
           * The pointer maths in `toCurveCoords` maps the whole element
           * rectangle onto the viewBox, which is only correct if the drawing
           * covers the element. With the default `xMidYMid meet`, an element
           * whose aspect ratio differs from 320:150 draws smaller and centred,
           * so the mapping drifts - points further right were read as much as
           * 4 units off, and the drag missed them.
           */
          preserveAspectRatio="none"
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
          {/* Axis labels: temperature across, duty up. */}
          <text x={PAD} y={H - 5} fill="currentColor" fillOpacity={0.4} fontSize={7}>0°C</text>
          <text x={W - PAD} y={H - 5} fill="currentColor" fillOpacity={0.4} fontSize={7} textAnchor="end">100°C</text>
          <text x={PAD - 4} y={H - PAD} fill="currentColor" fillOpacity={0.4} fontSize={7} textAnchor="end">0%</text>
          <text x={PAD - 4} y={PAD + 4} fill="currentColor" fillOpacity={0.4} fontSize={7} textAnchor="end">100%</text>

          {series.map((s) => (
            <g key={s.key}>
              <path
                d={toPath(s.points)}
                fill="none"
                stroke={s.color}
                strokeWidth={dragging?.channel === s.key ? 2.5 : 1.75}
                strokeLinecap="round"
                strokeLinejoin="round"
                style={{ transition: dragging === null ? "stroke 500ms ease" : undefined }}
              />
              {s.points.map((p, i) => {
                const x = PAD + (p.temp / 100) * (W - 2 * PAD);
                const y = H - PAD - (p.duty_pct / 100) * (H - 2 * PAD);
                const active = dragging?.channel === s.key && dragging.index === i;
                const movable = isEditablePoint(i);
                return (
                  <g key={`${s.key}-${i}`}>
                    {/*
                     * The first and last points are the EC's: command 14 does
                     * not carry them, so they are drawn hollow and grey to say
                     * "shown for shape, not yours to move".
                     */}
                    <circle
                      cx={x}
                      cy={y}
                      r={active ? 5 : movable ? 3 : 2.5}
                      fill={active ? "white" : movable ? s.color : "none"}
                      stroke={movable ? s.color : "currentColor"}
                      strokeOpacity={movable ? 1 : 0.35}
                      strokeWidth={active ? 2 : movable ? 1 : 1}
                    />
                    {/* While dragging, name the point being moved: with two
                        curves it is otherwise easy to grab the wrong one. */}
                    {active && (
                      <g>
                        <rect
                          x={x + 7}
                          y={y - 15}
                          width={s.label.length * 7 + 44}
                          height={16}
                          rx={2}
                          fill="rgba(0,0,0,0.8)"
                          stroke={s.color}
                          strokeWidth={0.75}
                        />
                        <text x={x + 12} y={y - 4} fill="#fff" fontSize={8}>
                          {s.label} · {p.temp}°C {p.duty_pct}%
                        </text>
                      </g>
                    )}
                  </g>
                );
              })}
            </g>
          ))}
        </Box>
      </Box>

      <CurveTable series={series} />

      {writable && (
        <Box sx={{ display: "flex", alignItems: "center", gap: 1, pt: 0.5, borderTop: "1px solid", borderTopColor: "divider" }}>
          <Button
            size="small"
            variant="contained"
            startIcon={busy ? undefined : <CheckIcon sx={{ fontSize: 15 }} />}
            disabled={!dirty || busy}
            onClick={apply}
            sx={{ textTransform: "none", minWidth: 96 }}
          >
            {busy ? "写入中…" : "应用曲线"}
          </Button>
          <Button
            size="small"
            startIcon={<RestoreIcon sx={{ fontSize: 14 }} />}
            disabled={!dirty || busy}
            onClick={reset}
            sx={{ textTransform: "none", color: "text.secondary" }}
          >
            重置
          </Button>
          <Typography sx={{ fontSize: "0.6875rem", color: "text.disabled", ml: "auto" }}>
            拖动任一端点编辑
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

/** The colour key: a swatch plus the fan name and what it controls. */
function Legend({
  palette,
  dirtyChannels,
}: {
  palette: ExtractedPalette;
  dirtyChannels: Channel[];
}) {
  const items: Array<{ channel: Channel; label: string; note: string; color: string }> = [
    { channel: "cpu", label: "CPU", note: "处理器风扇", color: rgbString(palette.primary) },
    { channel: "gpu1", label: "GPU1", note: "显卡风扇", color: rgbString(palette.secondary) },
  ];
  return (
    <Box sx={{ display: "flex", alignItems: "center", gap: 2, flexWrap: "wrap" }}>
      {items.map((item) => (
        <Box key={item.channel} sx={{ display: "flex", alignItems: "center", gap: 0.75 }}>
          <Box
            sx={{
              width: 18,
              height: 3,
              borderRadius: 0.5,
              backgroundColor: item.color,
              position: "relative",
              "&::after": {
                content: '""',
                position: "absolute",
                left: 8,
                top: -2.5,
                width: 8,
                height: 8,
                borderRadius: "50%",
                backgroundColor: item.color,
              },
            }}
          />
          <Typography sx={{ fontSize: "0.6875rem", fontWeight: 600, color: item.color }}>
            {item.label}
          </Typography>
          <Typography sx={{ fontSize: "0.625rem", color: "text.disabled" }}>
            {item.note}
          </Typography>
          {dirtyChannels.includes(item.channel) && (
            <Typography
              sx={{
                fontSize: "0.5625rem",
                color: "warning.main",
                border: "1px solid",
                borderColor: "warning.main",
                borderRadius: 0.5,
                px: 0.5,
                lineHeight: 1.4,
              }}
            >
              已改
            </Typography>
          )}
        </Box>
      ))}
    </Box>
  );
}

/**
 * A small per-fan table of the curve points.
 *
 * Easier to scan than a run of "(temp,duty)" pairs: one row per fan, one column
 * per point, and the first cell carries the same colour as the line.
 */
function CurveTable({ series }: { series: Series[] }) {
  const columns = series[0]?.points.length ?? 0;
  return (
    <Box
      component="table"
      sx={{
        width: "100%",
        borderCollapse: "collapse",
        tableLayout: "fixed",
        fontSize: "0.6875rem",
        "& th": {
          textAlign: "right",
          fontWeight: 500,
          color: "text.disabled",
          fontSize: "0.625rem",
          textTransform: "uppercase",
          letterSpacing: "0.08em",
          pb: 0.25,
        },
        "& td": { textAlign: "right", py: 0.4 },
      }}
    >
      <Box component="thead">
        <Box component="tr">
          <Box component="th" sx={{ textAlign: "left", width: 56, pl: 0.75 }}>
            风扇
          </Box>
          {Array.from({ length: columns }, (_, i) => (
            <Box component="th" key={i}>
              <Box component="span" sx={{ mr: 0.5 }}>
                点{i + 1}
              </Box>
              {/*
               * Only the middle points are written by command 14; the EC keeps
               * its own first and last, so those columns are marked as fixed.
               */}
              {!isEditablePoint(i) && (
                <Box
                  component="span"
                  sx={{
                    textTransform: "none",
                    letterSpacing: 0,
                    border: "1px solid",
                    borderColor: "divider",
                    borderRadius: 0.5,
                    px: 0.375,
                    fontSize: "0.5625rem",
                  }}
                >
                  固件固定
                </Box>
              )}
            </Box>
          ))}
        </Box>
      </Box>
      <Box component="tbody">
        {series.map((s) => (
          <Box
            component="tr"
            key={s.key}
            sx={{
              "& td:first-of-type": {
                borderLeft: "3px solid",
                borderLeftColor: s.color,
                pl: 0.75,
              },
              "&:not(:last-of-type) td": {
                borderBottom: "1px solid",
                borderBottomColor: "divider",
              },
            }}
          >
            <Box component="td" sx={{ textAlign: "left", color: s.color, fontWeight: 700 }}>
              {s.label}
            </Box>
            {s.points.map((p, i) => (
              <Box component="td" key={i} sx={{ opacity: isEditablePoint(i) ? 1 : 0.5 }}>
                <Box component="span" sx={{ color: "text.disabled", mr: 0.75 }}>
                  {p.temp}°C
                </Box>
                <Box component="span" sx={{ fontWeight: 600, color: "text.primary" }}>
                  {p.duty_pct}%
                </Box>
              </Box>
            ))}
          </Box>
        ))}
      </Box>
    </Box>
  );
}
