import { describe, expect, it } from "vitest";
import { fanModeName, perfModeName } from "./daemon";

describe("mode name helpers", () => {
  it("maps known fan modes", () => {
    expect(fanModeName(0)).toBe("auto");
    expect(fanModeName(1)).toBe("max");
    expect(fanModeName(5)).toBe("maxq");
    expect(fanModeName(6)).toBe("custom");
    expect(fanModeName(8)).toBe("quiet");
  });

  it("maps an unset fan mode to an explicit unknown", () => {
    expect(fanModeName(255)).toBe("unknown (not set)");
  });

  it("reports unknown values rather than guessing", () => {
    expect(fanModeName(42)).toBe("unknown (42)");
  });

  it("maps known performance modes", () => {
    expect(perfModeName(0)).toBe("quiet");
    expect(perfModeName(1)).toBe("pwrsaving");
    expect(perfModeName(2)).toBe("performance");
    expect(perfModeName(3)).toBe("entertainment");
    expect(perfModeName(255)).toBe("unknown (not set)");
  });
});

describe("mode choices", () => {
  it("expose the write-side values the daemon accepts", async () => {
    const { FAN_MODE_CHOICES, PERF_MODE_CHOICES, fanModeName, perfModeName } =
      await import("./daemon");
    for (const choice of FAN_MODE_CHOICES) {
      expect(fanModeName(choice.value)).toBe(choice.label);
    }
    for (const choice of PERF_MODE_CHOICES) {
      expect(perfModeName(choice.value)).toBe(choice.label);
    }
  });

  it("lists every fan mode the driver accepts", async () => {
    const { FAN_MODE_CHOICES } = await import("./daemon");
    const labels = FAN_MODE_CHOICES.map((c) => c.label).sort();
    expect(labels).toEqual(["auto", "custom", "max", "maxq", "quiet"]);
  });
});
