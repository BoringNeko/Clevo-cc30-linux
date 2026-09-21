import { useEffect, useState } from "react";

import {
  designScale,
  designSize,
  DESIGN_HEIGHT,
  DESIGN_WIDTH,
  type AspectRatio,
} from "../theme";

/** The current viewport in CSS pixels. */
export interface Viewport {
  width: number;
  height: number;
}

/** The design surface size for the active ratio, in design pixels. */
export interface DesignSize {
  width: number;
  height: number;
}

/** Read the current viewport, or a sensible default outside a browser. */
function viewport(): Viewport {
  if (typeof window === "undefined") {
    return { width: DESIGN_WIDTH, height: DESIGN_HEIGHT };
  }
  return { width: window.innerWidth, height: window.innerHeight };
}

/**
 * Track the viewport and report the factor that maps the design space onto it,
 * together with the design surface size for the active aspect ratio.
 *
 * The interface is laid out at a fixed design size — 1600x900 for 16:9,
 * 1600x1000 for 16:10 — and then scaled by this factor, so no window size can
 * make it scroll. Following the ratio means a 16:10 window fills its full height
 * instead of showing black bands above and below a 16:9 surface.
 *
 * `ResizeObserver` on the document element is used rather than `window.resize`
 * because the viewport can change without a window resize (for example when the
 * system scale factor changes).
 */
export function useDesignScale(aspect: AspectRatio = "16:9"): {
  scale: number;
  viewport: Viewport;
  design: DesignSize;
} {
  const [size, setSize] = useState<Viewport>(viewport);
  const design = designSize(aspect);

  useEffect(() => {
    const update = () => setSize(viewport());
    update();

    window.addEventListener("resize", update);
    let observer: ResizeObserver | undefined;
    if (typeof ResizeObserver !== "undefined") {
      observer = new ResizeObserver(update);
      observer.observe(document.documentElement);
    }
    return () => {
      window.removeEventListener("resize", update);
      observer?.disconnect();
    };
  }, []);

  return {
    scale: designScale(size.width, size.height, design.width, design.height),
    viewport: size,
    design,
  };
}
