import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";
import CheckIcon from "@mui/icons-material/Check";
import SettingsBackupRestoreIcon from "@mui/icons-material/SettingsBackupRestore";
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
  /** Live temperatures, for the readout above the chart. */
  temps?: Partial<Record<Channel, number>>;
}

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

/**
 * The curve the machine shipped with.
 *
 * Read from the EC before anything was written (see `docs/hardware-notes.md`)
 * and kept here so "还原默认" has something to restore. Duty is a percentage,
 * as everywhere above `clevo-proto`.
 */
export const FACTORY_CURVE: FanCurve = {
  fan_count: 2,
  init_mode: 0,
  kb_type: 6,
  cpu: [
    { temp: 40, duty_pct: 25 },
    { temp: 60, duty_pct: 36 },
    { temp: 80, duty_pct: 53 },
    { temp: 100, duty_pct: 100 },
  ],
  gpu1: [
    { temp: 40, duty_pct: 25 },
    { temp: 60, duty_pct: 36 },
    { temp: 80, duty_pct: 53 },
    { temp: 99, duty_pct: 100 },
  ],
  gpu2: [
    { temp: 0, duty_pct: 0 },
    { temp: 0, duty_pct: 0 },
    { temp: 0, duty_pct: 0 },
    { temp: 0, duty_pct: 0 },
  ],
};

/** The two fans the firmware writes through command 14. */
type Channel = "cpu" | "gpu1";

const CHANNELS: readonly Channel[] = ["cpu", "gpu1"];

/** The chart's inner height, in px. */
const CHART_H = 160;
/** Width of the Y-axis gutter to the left of the chart, in px. */
const AXIS_W = 32;
/** Gap between the gutter and the chart, in px (MUI spacing 1). */
const AXIS_GAP = 8;
/** Chart padding, as a fraction of the chart box that is not used. */
const PAD_PCT = 4;

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

const MIN_GAP_C = 2;

/**
 * The duty a curve applies at `temp`, linearly interpolated between points.
 *
 * Used by the live readout so the number above the chart matches what the
 * firmware would do at the reported temperature, rather than the nearest point.
 */
export function dutyAtTemp(temp: number, points: CurvePoint[]): number {
  if (points.length === 0) return 0;
  if (temp <= points[0].temp) return points[0].duty_pct;
  for (let i = 0; i < points.length - 1; i++) {
    const a = points[i];
    const b = points[i + 1];
    if (temp >= a.temp && temp <= b.temp) {
      if (b.temp === a.temp) return a.duty_pct;
      const ratio = (temp - a.temp) / (b.temp - a.temp);
      return Math.round(a.duty_pct + ratio * (b.duty_pct - a.duty_pct));
    }
  }
  return points[points.length - 1].duty_pct;
}

/** The duty axis is always the full 0..100 %: duty is absolute, not relative. */
const DUTY_RANGE: Range = { lo: 0, hi: 100 };

/** A closed span on one axis of the plot. */
export interface Range {
  lo: number;
  hi: number;
}

/**
 * The span of temperatures the plot x-axis covers.
 *
 * The curve only exists between its first and last point (40..100 °C on this
 * machine), so plotting it against a fixed 0..100 scale leaves the whole left
 * side of the chart empty and squeezes the curve into the right third. The axis
 * therefore follows the curve: `lo` and `hi` are the outermost temperatures the
 * two fans actually use, shared so both lines land on the same two values.
 */
export type TempRange = Range;

/**
 * The range covering `values`, widened to a minimum span so a degenerate curve
 * cannot divide by zero.
 */
function rangeOf(values: number[], fallback: Range): Range {
  if (values.length === 0) return fallback;
  const lo = Math.min(...values);
  const hi = Math.max(lo + 1, Math.max(...values));
  return { lo, hi };
}

/** The temperature span covered by `series`, over both fans. */
export function tempRangeOf(series: CurvePoint[][]): TempRange {
  return rangeOf(series.flat().map((p) => p.temp), { lo: 0, hi: 100 });
}

/** Map a value in `range` to 0..100 across the plot's data area. */
function toPct(value: number, range: Range): number {
  return ((value - range.lo) / (range.hi - range.lo)) * 100;
}

/** The inverse of {@link toPct}. */
function fromPct(pct: number, range: Range): number {
  return range.lo + (pct / 100) * (range.hi - range.lo);
}

/** Map a duty percentage to a chart y, in percent from the top of the chart. */
function dutyToY(duty: number, range: Range): number {
  return 100 - toPct(duty, range);
}

/**
 * The smooth cubic path through the points, in the chart's 0..100 space.
 *
 * The reference draws each segment as a curve whose control points sit
 * horizontally between the two endpoints, which removes the corners a straight
 * polyline leaves while still passing exactly through every point.
 */
export function curvePath(
  points: CurvePoint[],
  inset = 0,
  tempRange: Range = { lo: 0, hi: 100 },
  dutyRange: Range = { lo: 0, hi: 100 },
): string {
  if (points.length === 0) return "";
  const lo = inset;
  const hi = 100 - inset;
  const span = hi - lo;
  const at = (p: CurvePoint) => ({
    x: lo + (toPct(p.temp, tempRange) / 100) * span,
    y: lo + (dutyToY(p.duty_pct, dutyRange) / 100) * span,
  });
  const first = at(points[0]);
  return points.slice(1).reduce((acc, curr, i) => {
    const prev = at(points[i]);
    const to = at(curr);
    const cx = (prev.x + to.x) / 2;
    return `${acc} C ${cx} ${prev.y}, ${cx} ${to.y}, ${to.x} ${to.y}`;
  }, `M ${first.x} ${first.y}`);
}

/** The two fans' names, colours and points, as the chart and readout need them. */
interface Series {
  key: Channel;
  label: string;
  color: string;
  points: CurvePoint[];
}

/**
 * The fan curve as two smooth lines (CPU and GPU1), both draggable.
 *
 * Editing is local until "保存配置" is pressed, so the user can shape both
 * curves and only then send them to the EC (a write costs a PolicyKit prompt).
 *
 * Layout follows the reference design: the live readout sits above the chart,
 * the duty scale lives in its own gutter to the left and the temperature scale
 * below, and the control points are square handles rather than flat markers.
 */
export function CurveCard({
  palette,
  curve,
  writable = false,
  onApplied,
  temps,
}: CurveCardProps) {
  const colors: Record<Channel, string> = {
    cpu: rgbString(palette.primary),
    gpu1: rgbString(palette.secondary),
  };

  const [draft, setDraft] = useState<FanCurve>(curve);
  const [dragging, setDragging] = useState<{ channel: Channel; index: number } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const chartRef = useRef<HTMLDivElement | null>(null);

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

  /**
   * The temperatures the x-axis spans, shared by both fans.
   *
   * Derived from the *daemon* curve rather than the draft: it must not shift
   * while a point is being dragged, or the point would slide out from under the
   * pointer.
   */
  const tempRange = useMemo(() => tempRangeOf([curve.cpu, curve.gpu1]), [curve.cpu, curve.gpu1]);
  /**
   * The duty axis stays a fixed 0..100 %.
   *
   * Scaling it to the curve (as the temperature axis is) would make a curve
   * whose duties span only 25..100 look steeper than it is, and duty is an
   * absolute quantity - 0 % means the fan is off, which the plot has to show.
   */
  const dutyRange = DUTY_RANGE;

  const series: Series[] = [
    {
      key: "cpu",
      label: "CPU",
      color: colors.cpu,
      points: draft.cpu,
    },
    {
      key: "gpu1",
      label: "GPU1",
      color: colors.gpu1,
      points: draft.gpu1,
    },
  ];

  /** Convert a pointer event into (temp, duty), both in their own units. */
  const toCurveCoords = useCallback(
    (event: { clientX: number; clientY: number }) => {
      const chart = chartRef.current;
      if (!chart) return null;
      const rect = chart.getBoundingClientRect();
      const span = 100 - 2 * PAD_PCT;
      const xPct = ((event.clientX - rect.left) / rect.width) * 100;
      const yPct = ((event.clientY - rect.top) / rect.height) * 100;
      return {
        temp: fromPct(((xPct - PAD_PCT) / span) * 100, tempRange),
        duty: fromPct(((100 - PAD_PCT - yPct) / span) * 100, dutyRange),
      };
    },
    [tempRange, dutyRange],
  );

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
    (event: { clientX: number; clientY: number }): { channel: Channel; index: number } | null => {
      const chart = chartRef.current;
      const coords = toCurveCoords(event);
      if (!chart || !coords) return null;
      const rect = chart.getBoundingClientRect();
      // The grab radius is a screen distance. The point is located in
      // percent-of-plot, so a percent difference converts to pixels by dividing
      // by the pixels-per-percent - multiplying would scale it the wrong way and
      // make every press land on a point.
      const pxPerPctX = rect.width / (100 - 2 * PAD_PCT);
      const pxPerPctY = rect.height / (100 - 2 * PAD_PCT);
      let best: { channel: Channel; index: number } | null = null;
      let bestDist = HIT_RADIUS_PX;
      CHANNELS.forEach((channel) => {
        EDITABLE_INDICES.forEach((i) => {
          const p = draft[channel][i];
          if (!p) return;
          const dx = (toPct(p.temp, tempRange) - toPct(coords.temp, tempRange)) * pxPerPctX;
          const dy = (toPct(p.duty_pct, dutyRange) - toPct(coords.duty, dutyRange)) * pxPerPctY;
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
    [draft, tempRange, dutyRange, toCurveCoords],
  );

  const onPointerDown = (event: React.PointerEvent<SVGSVGElement>) => {
    if (!writable) return;
    const hit = nearestPoint(event);
    if (hit === null) return;
    /*
     * Stop the browser's default handling of a press-drag.
     *
     * Without this the gesture becomes a native text selection: the pointer
     * sweeps across the axis labels below the plot, selects them, and the drag
     * turns into "dragging the selection" - the curve loses the pointer and the
     * point stops following. `user-select: none` on the chart backs this up, but
     * the default action has to be cancelled too, otherwise the browser still
     * starts a selection at the press.
     */
    event.preventDefault();
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

  /**
   * Put the edited curve back to what the EC currently holds (discard edits).
   *
   * Named 还原配置 in the UI: this undoes local editing, it does not write.
   */
  const restoreSaved = () => {
    setDraft({ ...draft, cpu: curve.cpu, gpu1: curve.gpu1 });
    adopted.current = { cpu: curve.cpu, gpu1: curve.gpu1 };
    setError(null);
    setNotice(null);
  };

  /**
   * Load the factory curve into the editor.
   *
   * Nothing is written until 保存配置 is pressed, so this is safe to try: the
   * user can see what the default looks like and still back out with 还原配置.
   */
  const restoreDefault = () => {
    setDraft({
      ...draft,
      cpu: FACTORY_CURVE.cpu.map((p) => ({ ...p })),
      gpu1: FACTORY_CURVE.gpu1.map((p) => ({ ...p })),
    });
    setError(null);
    setNotice("已载入出厂默认曲线，按「保存配置」写入");
  };

  return (
    /*
     * Nothing in this card is meant to be selected: the readout and the drag
     * label are live values, and a drag that sweeps across them must keep
     * moving the point rather than start a text selection.
     */
    <GlassCard sx={{ gap: 1.5, userSelect: "none", WebkitUserSelect: "none" }}>
      <CardHeader
        icon={<ShowChartIcon sx={{ fontSize: 16 }} />}
        title="风扇曲线"
        hint={dirty ? "未应用" : undefined}
      />

      <Readout series={series} dragging={dragging} temps={temps} />

      {/* The chart: a duty gutter on the left, the plot on the right. */}
      <Box sx={{ display: "flex", flexDirection: "column", userSelect: "none" }}>
        <Box sx={{ display: "flex", height: CHART_H }}>
          <YAxis range={dutyRange} />
          <Box
            ref={chartRef}
            sx={{
              flex: 1,
              minWidth: 0,
              position: "relative",
              borderRadius: 1,
              border: "1px solid", borderColor: "divider",
              backgroundColor: "action.hover",
              // A drag across the plot must move the point, never select the
              // axis labels or the drag label it passes over.
              userSelect: "none",
              WebkitUserSelect: "none",
            }}
          >
            <Box
              component="svg"
              viewBox="0 0 100 100"
              /*
               * Stretch to fill the box rather than letterboxing it.
               *
               * The pointer maths in `toCurveCoords` maps the whole element
               * rectangle onto the viewBox, which is only correct if the drawing
               * covers the element. With the default `xMidYMid meet`, an element
               * whose aspect ratio differs from the viewBox draws smaller and
               * centred, so the mapping drifts - points further right were read
               * as much as 4 units off, and the drag missed them.
               */
              preserveAspectRatio="none"
              onPointerDown={onPointerDown}
              onPointerMove={onPointerMove}
              onPointerUp={endDrag}
              onPointerCancel={endDrag}
              sx={{
                width: "100%",
                height: "100%",
                display: "block",
                color: "text.primary",
                touchAction: "none",
                overflow: "visible",
                cursor: writable ? (dragging === null ? "crosshair" : "grabbing") : "default",
              }}
            >
              {/* Horizontal grid, so a duty can be read off the curve. */}
              {[25, 50, 75].map((y) => (
                <line
                  key={y}
                  x1={0}
                  y1={y}
                  x2={100}
                  y2={y}
                  stroke="currentColor"
                  strokeOpacity={0.08}
                  strokeWidth={1}
                  strokeDasharray="2 3"
                  vectorEffect="non-scaling-stroke"
                />
              ))}

              {series.map((s) => (
                <path
                  key={`${s.key}-line`}
                  data-testid={`curve-line-${s.key}`}
                  d={curvePath(s.points, PAD_PCT, tempRange, dutyRange)}
                  fill="none"
                  stroke={s.color}
                  strokeWidth={dragging?.channel === s.key ? 2.25 : 1.75}
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  vectorEffect="non-scaling-stroke"
                  style={{ transition: dragging === null ? "stroke 500ms ease" : undefined }}
                />
              ))}
            </Box>

            {/*
             * Handles sit above the plot as square buttons, matching the
             * reference; the SVG underneath still owns the pointer events.
             *
             * Only the points command 14 carries get one. The first and last
             * belong to the EC, so a marker there would invite a drag that
             * silently does nothing - the curve still runs through them, which
             * is what gives it its shape.
             */}
            {series.map((s, layer) =>
              s.points.map((p, i) => {
                if (!isEditablePoint(i)) return null;
                const active = dragging?.channel === s.key && dragging.index === i;
                return (
                  <Handle
                    key={`${s.key}-${i}`}
                    x={PAD_PCT + (toPct(p.temp, tempRange) / 100) * (100 - 2 * PAD_PCT)}
                    y={PAD_PCT + (dutyToY(p.duty_pct, dutyRange) / 100) * (100 - 2 * PAD_PCT)}
                    color={s.color}
                    active={active}
                    /* The later series is drawn on top, which is also the
                       series the hit test prefers on a tie. */
                    layer={layer}
                    label={`${s.label} · ${p.temp}°C ${p.duty_pct}%`}
                  />
                );
              }),
            )}
          </Box>
        </Box>

        <XAxis range={tempRange} />
      </Box>

      {writable && (
        <Box sx={{ display: "flex", alignItems: "center", gap: 1, pt: 0.5, borderTop: "1px solid", borderTopColor: "divider" }}>
          <Box
            component="button"
            type="button"
            disabled={!dirty || busy}
            onClick={apply}
            sx={{
              display: "inline-flex",
              alignItems: "center",
              gap: 0.75,
              px: 1.25,
              py: 0.75,
              minWidth: 96,
              justifyContent: "center",
              borderRadius: 0.75,
              border: "1px solid", borderColor: "divider",
              backgroundColor: dirty && !busy ? "rgb(var(--accent) / 0.32)" : "action.hover",
              color: dirty && !busy ? "#fff" : "text.disabled",
              font: "inherit",
              fontSize: "0.75rem",
              letterSpacing: "0.02em",
              cursor: dirty && !busy ? "pointer" : "not-allowed",
              transition: "background-color 300ms ease, border-color 300ms ease, color 300ms ease",
              "&:hover:not(:disabled)": { backgroundColor: "rgb(var(--accent) / 0.42)" },
            }}
          >
            {busy ? null : <CheckIcon sx={{ fontSize: 15 }} />}
            {busy ? "写入中…" : "保存配置"}
          </Box>
          {/*
           * Two different "undo"s: 还原配置 goes back to what the EC holds
           * (discarding local edits), 还原默认 loads the factory curve into the
           * editor. Neither writes - 保存配置 is still the only button that
           * touches the hardware.
           */}
          <SecondaryButton
            icon={<RestoreIcon sx={{ fontSize: 14 }} />}
            label="还原配置"
            disabled={!dirty || busy}
            onClick={restoreSaved}
          />
          <SecondaryButton
            icon={<SettingsBackupRestoreIcon sx={{ fontSize: 14 }} />}
            label="还原默认"
            disabled={busy}
            onClick={restoreDefault}
          />
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

/** Pointer distance, in screen px, that counts as grabbing a handle. */
const HIT_RADIUS_PX = 16;

/** A flat secondary action, per the reference's "Reset default" treatment. */
function SecondaryButton({
  icon,
  label,
  disabled,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  disabled: boolean;
  onClick: () => void;
}) {
  return (
    <Box
      component="button"
      type="button"
      disabled={disabled}
      onClick={onClick}
      sx={{
        display: "inline-flex",
        alignItems: "center",
        gap: 0.75,
        px: 0.75,
        py: 0.75,
        borderRadius: 0.75,
        border: "1px solid transparent",
        backgroundColor: "transparent",
        color: "text.secondary",
        font: "inherit",
        fontSize: "0.75rem",
        letterSpacing: "0.02em",
        cursor: disabled ? "not-allowed" : "pointer",
        opacity: disabled ? 0.4 : 1,
        transition: "color 300ms ease, background-color 300ms ease",
        "&:hover:enabled": { backgroundColor: "action.hover", color: "text.primary" },
      }}
    >
      {icon}
      {label}
    </Box>
  );
}

/**
 * The live readout above the chart.
 *
 * Shows the current temperature and the duty the draft curve would apply
 * there. While a handle is held the pair switches to that point's own values,
 * which is what makes the drag legible without a tooltip.
 */
function Readout({
  series,
  dragging,
  temps,
}: {
  series: Series[];
  dragging: { channel: Channel; index: number } | null;
  temps?: Partial<Record<Channel, number>>;
}) {
  return (
    <Box data-testid="curve-readout" sx={{ display: "flex", gap: 4 }}>
      {series.map((s) => {
        const held = dragging?.channel === s.key ? s.points[dragging.index] : undefined;
        const temp = held ? held.temp : temps?.[s.key];
        const duty = held ? held.duty_pct : temp !== undefined ? dutyAtTemp(temp, s.points) : undefined;
        return (
          <Box key={s.key}>
            <Typography
              sx={{
                fontSize: "0.6875rem",
                fontWeight: 500,
                letterSpacing: "0.12em",
                textTransform: "uppercase",
                color: "text.disabled",
                mb: 0.5,
              }}
            >
              {s.label} 风扇
            </Typography>
            <Box sx={{ display: "flex", alignItems: "baseline", gap: 0.5 }}>
              <Typography
                sx={{
                  fontSize: "1.5rem",
                  fontWeight: 600,
                  color: "text.primary",
                  fontVariantNumeric: "tabular-nums",
                }}
              >
                {temp !== undefined ? `${temp}°C` : "—"}
              </Typography>
              <Typography
                sx={{
                  fontSize: "0.875rem",
                  fontWeight: 600,
                  color: s.color,
                  fontVariantNumeric: "tabular-nums",
                  ml: 1,
                }}
              >
                {duty !== undefined ? `${duty}%` : "—"}
              </Typography>
            </Box>
          </Box>
        );
      })}
    </Box>
  );
}

/**
 * The duty scale, in its own gutter to the left of the plot.
 *
 * Like the temperature scale, the labels are the real values the axis covers
 * rather than a fixed 0..100, so the curve's own ends sit against the labels
 * they belong to.
 */
function YAxis({ range }: { range: Range }) {
  const ticks = 4;
  const step = (range.hi - range.lo) / ticks;
  // Top-down, so the highest duty is the topmost label.
  const labels = Array.from({ length: ticks + 1 }, (_, i) => Math.round(range.hi - i * step));
  return (
    <Box sx={{ width: AXIS_W, position: "relative", mr: 1, flexShrink: 0 }}>
      {labels.map((val) => (
        <Typography
          key={val}
          sx={{
            position: "absolute",
            top: `${PAD_PCT + (dutyToY(val, range) / 100) * (100 - 2 * PAD_PCT)}%`,
            right: 0,
            transform: "translateY(-50%)",
            fontSize: "0.625rem",
            letterSpacing: "0.12em",
            color: "text.disabled",
            fontVariantNumeric: "tabular-nums",
          }}
        >
          {val}%
        </Typography>
      ))}
    </Box>
  );
}

/**
 * The temperature scale, aligned to the plot's inner (data) area.
 *
 * The labels are the real temperatures the axis covers - the curve's own first
 * and last point - not a fixed 0..100, so the ends of the curve sit exactly
 * under the values they represent.
 */
function XAxis({ range }: { range: TempRange }) {
  const ticks = 4;
  const span = 100 - 2 * PAD_PCT;
  const step = (range.hi - range.lo) / ticks;
  const labels = Array.from({ length: ticks + 1 }, (_, i) => Math.round(range.lo + i * step));
  return (
    <Box sx={{ display: "flex", ml: `${AXIS_W + AXIS_GAP}px`, mt: 1 }}>
      <Box sx={{ position: "relative", flex: 1, height: 15 }}>
        {labels.map((t, i) => (
          <Typography
            key={t}
            sx={{
              position: "absolute",
              // The label marks where the value sits in the data area, which is
              // inset from the box by PAD_PCT on each side - not the box edges.
              left: `${PAD_PCT + (i / ticks) * span}%`,
              transform: "translateX(-50%)",
              fontSize: "0.625rem",
              letterSpacing: "0.12em",
              color: "text.disabled",
              textTransform: "uppercase",
              whiteSpace: "nowrap",
            }}
          >
            {t}°C
          </Typography>
        ))}
      </Box>
    </Box>
  );
}

/**
 * One control handle.
 *
 * Square with a 6px radius and a white rim, per the reference. Only the two
 * points command 14 carries get one, so there is nothing here but an editable
 * point. The pointer events are handled by the SVG underneath, which is why
 * this is inert.
 */
function Handle({
  x,
  y,
  color,
  active,
  layer,
  label,
}: {
  x: number;
  y: number;
  color: string;
  active: boolean;
  layer: number;
  label: string;
}) {
  return (
    <Box
      data-testid="curve-handle"
      sx={{
        position: "absolute",
        left: `${x}%`,
        top: `${y}%`,
        width: 14,
        height: 14,
        borderRadius: 0.75,
        backgroundColor: color,
        border: "2px solid rgba(255,255,255,0.9)",
        boxShadow: active ? "0 4px 12px rgba(0,0,0,0.5)" : "0 2px 6px rgba(0,0,0,0.4)",
        /*
         * `left`/`top` place the box's top-left corner on the point; the
         * translate re-centres it, so the handle is centred on the value.
         */
        transform: `translate(-50%, -50%)${active ? " scale(1.15)" : ""}`,
        zIndex: 20 - layer,
        transition: active ? "none" : "transform 150ms ease",
        pointerEvents: "none",
      }}
    >
      {active && (
        <Box
          sx={{
            position: "absolute",
            left: 12,
            /* Below the handle when it is near the top, so the label stays in
               the plot instead of colliding with the readout above it. */
            top: y < 18 ? 18 : -14,
            whiteSpace: "nowrap",
            px: 0.5,
            py: 0.25,
            borderRadius: 0.5,
            border: "1px solid",
            borderColor: color,
            backgroundColor: "rgba(0,0,0,0.8)",
            fontSize: "0.5625rem",
            color: "#fff",
            fontVariantNumeric: "tabular-nums",
          }}
        >
          {label}
        </Box>
      )}
    </Box>
  );
}
