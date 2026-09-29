import { useEffect, useRef } from "react";
import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";
import type { KeyboardMode } from "../api/daemon";
import { effectFrame, isAnimated, type EffectFrame } from "../lib/keyboardEffect";
import { KEYBOARD_LAYOUT, KEY_GAP_PX, KEY_UNIT_PX } from "../lib/keyboardLayout";
import type { RGB } from "../lib/color";

interface KeyboardStageProps {
  mode: KeyboardMode;
  color: RGB;
  brightness: number;
  /** Pause the animation while the user is editing (keeps the board readable). */
  paused?: boolean;
}

/**
 * Single-zone keyboard visualisation.
 *
 * Shows the board and the lightguide underglow driven by the same frame, so it
 * reflects the synchronised single channel rather than implying independent
 * keys. The animation is a *preview* of the firmware effect; the EC runs the
 * real thing.
 *
 * The frame is applied by writing CSS custom properties on the root, not by
 * re-rendering: a 90-key board repainted through React at 60fps would churn the
 * reconciler for no reason, and the reference studio uses the same trick.
 */
export function KeyboardStage({ mode, color, brightness, paused = false }: KeyboardStageProps) {
  const rootRef = useRef<HTMLDivElement>(null);
  const pressedRef = useRef<string | null>(null);

  // The frame at t=0, used for the initial CSS variables so the first paint is
  // correct rather than one frame of black.
  const initial = effectFrame(mode, color, 0);

  useEffect(() => {
    const el = rootRef.current;
    if (!el) return;

    const apply = (frame: EffectFrame) => {
      el.style.setProperty("--kb-r", String(frame.rgb[0]));
      el.style.setProperty("--kb-g", String(frame.rgb[1]));
      el.style.setProperty("--kb-b", String(frame.rgb[2]));
      const glow = Math.max(0, Math.min(1, (brightness / 100) * frame.intensity));
      el.style.setProperty("--kb-glow", glow.toFixed(3));
    };

    const staticMode = !isAnimated(mode);
    if (paused || staticMode) {
      apply(effectFrame(mode, color, 0));
      return;
    }

    const startedAt = performance.now();
    let raf = 0;
    const tick = () => {
      apply(effectFrame(mode, color, (performance.now() - startedAt) / 1000));
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [paused, mode, color, brightness]);

  // A physical keypress lights the matching cap. In a single-zone board the
  // whole surface reacts, so this only supplies the tactile feedback.
  useEffect(() => {
    const down = (event: KeyboardEvent) => {
      const el = rootRef.current;
      if (!el) return;
      const previous = el.querySelector<HTMLElement>("[data-kb-pressed='true']");
      previous?.removeAttribute("data-kb-pressed");
      const cap = el.querySelector<HTMLElement>(`[data-kb-code='${event.code}']`);
      cap?.setAttribute("data-kb-pressed", "true");
      pressedRef.current = event.code;
      window.setTimeout(() => cap?.removeAttribute("data-kb-pressed"), 110);
    };
    window.addEventListener("keydown", down);
    return () => window.removeEventListener("keydown", down);
  }, []);

  const [ir, ig, ib] = initial.rgb;
  const boardWidth = 15 * KEY_UNIT_PX + 14 * KEY_GAP_PX;

  return (
    <Box
      ref={rootRef}
      sx={{
        // Defaults mirror the t=0 frame; the effect overwrites them each tick.
        "--kb-r": ir,
        "--kb-g": ig,
        "--kb-b": ib,
        "--kb-glow": "0",
        position: "relative",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        overflow: "hidden",
        px: 2,
        pt: 3,
        pb: 2,
        borderRadius: 1,
        background: "radial-gradient(circle at 50% 30%, rgba(255,255,255,0.05), rgba(0,0,0,0.35))",
        border: "1px solid",
        borderColor: "divider",
      }}
    >
      <Box sx={{ position: "relative", width: boardWidth, maxWidth: "100%" }}>
        {/* Desk underglow: the light spilling past the chassis. */}
        <Box
          aria-hidden
          sx={{
            position: "absolute",
            inset: -26,
            borderRadius: 2,
            filter: "blur(38px)",
            backgroundColor: "rgb(var(--kb-r), var(--kb-g), var(--kb-b))",
            opacity: "calc(var(--kb-glow) * 0.7)",
            pointerEvents: "none",
          }}
        />
        {/* Chassis. */}
        <Box
          sx={{
            position: "relative",
            p: 1,
            borderRadius: 1.5,
            background: "linear-gradient(180deg, #23262f 0%, #14161b 100%)",
            border:
              "1.5px solid rgba(var(--kb-r), var(--kb-g), var(--kb-b), calc(var(--kb-glow) * 0.5 + 0.12))",
            boxShadow:
              "0 18px 40px -18px rgba(0,0,0,0.9), 0 0 16px rgba(var(--kb-r), var(--kb-g), var(--kb-b), calc(var(--kb-glow) * 0.35))",
          }}
        >
          {/* Diffusion plate behind the keys. */}
          <Box
            aria-hidden
            sx={{
              position: "absolute",
              inset: 6,
              borderRadius: 1,
              background:
                "radial-gradient(ellipse at center, rgba(var(--kb-r), var(--kb-g), var(--kb-b), calc(var(--kb-glow) * 0.9)) 0%, rgba(var(--kb-r), var(--kb-g), var(--kb-b), calc(var(--kb-glow) * 0.55)) 100%)",
              filter: "blur(1.5px)",
              pointerEvents: "none",
            }}
          />
          <Box sx={{ position: "relative", display: "flex", flexDirection: "column", gap: `${KEY_GAP_PX}px` }}>
            {KEYBOARD_LAYOUT.map((row, rowIndex) => (
              <Box key={rowIndex} sx={{ display: "flex", gap: `${KEY_GAP_PX}px` }}>
                {row.map((key) => (
                  <Box
                    key={key.code}
                    data-kb-code={key.code}
                    sx={{
                      width: key.w * KEY_UNIT_PX + (key.w - 1) * KEY_GAP_PX,
                      height: KEY_UNIT_PX * 0.92,
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "center",
                      borderRadius: 0.75,
                      background: "linear-gradient(180deg, #2b303c 0%, #1c1f26 100%)",
                      boxShadow:
                        "0 3px 0 0 #101216, 0 4px 6px -1px rgba(0,0,0,0.7), inset 0 1px 0 rgba(255,255,255,0.12)",
                      transition: "transform 60ms ease, box-shadow 60ms ease",
                      "&[data-kb-pressed='true']": {
                        transform: "translateY(2px)",
                        boxShadow: "inset 0 0 8px rgba(0,0,0,0.8)",
                      },
                    }}
                  >
                    <Typography
                      sx={{
                        fontFamily: "'JetBrains Mono', ui-monospace, monospace",
                        fontSize: "0.5625rem",
                        fontWeight: 600,
                        color: "rgba(255,255,255, calc(0.55 + var(--kb-glow) * 0.4))",
                        textShadow:
                          "0 0 5px rgba(var(--kb-r), var(--kb-g), var(--kb-b), calc(var(--kb-glow) * 0.9))",
                        userSelect: "none",
                      }}
                    >
                      {key.label}
                    </Typography>
                  </Box>
                ))}
              </Box>
            ))}
          </Box>
        </Box>
      </Box>
    </Box>
  );
}
