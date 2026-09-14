import { describe, expect, it } from "vitest";
import { FreshnessBadge } from "./FreshnessBadge";

// Minimal render check without a DOM testing library: React can render to a
// string via react-dom/server.
import { renderToStaticMarkup } from "react-dom/server";

describe("FreshnessBadge", () => {
  it("labels fresh as 实时", () => {
    const html = renderToStaticMarkup(<FreshnessBadge freshness="fresh" />);
    expect(html).toContain("实时");
    expect(html).toContain("data-testid=\"freshness\"");
  });

  it("labels stale distinctly so it is not mistaken for fresh", () => {
    const html = renderToStaticMarkup(<FreshnessBadge freshness="stale" />);
    expect(html).toContain("已过期");
    expect(html).not.toContain(">实时<");
  });

  it("labels unknown", () => {
    const html = renderToStaticMarkup(<FreshnessBadge freshness="unknown" />);
    expect(html).toContain("未知");
  });
});
