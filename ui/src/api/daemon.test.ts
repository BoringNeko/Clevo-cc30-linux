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
  it("expose a wire name the daemon accepts for every choice", async () => {
    const { FAN_MODE_CHOICES, PERF_MODE_CHOICES } = await import("./daemon");
    // The daemon accepts these names; the button may show a friendlier label.
    const fanWireNames = ["auto", "quiet", "maxq", "max", "custom"];
    expect(FAN_MODE_CHOICES.map((c) => c.mode).sort()).toEqual(
      [...fanWireNames].sort(),
    );
    for (const choice of FAN_MODE_CHOICES) {
      expect(fanModeName(choice.value)).toBe(choice.mode);
    }
    for (const choice of PERF_MODE_CHOICES) {
      expect(perfModeName(choice.value)).toBe(choice.label);
    }
  });

  it("lists every fan mode the driver accepts, under its UI label", async () => {
    const { FAN_MODE_CHOICES } = await import("./daemon");
    const labels = FAN_MODE_CHOICES.map((c) => c.label).sort();
    expect(labels).toEqual(["auto", "customize", "max", "maxq", "quiet"]);
  });

  it("labels the curve mode `customize` while writing `custom`", async () => {
    const { FAN_MODE_CHOICES, CUSTOMIZE_FAN_MODE, isCustomizeMode } =
      await import("./daemon");
    const choice = FAN_MODE_CHOICES.find((c) => c.value === CUSTOMIZE_FAN_MODE);
    expect(choice?.label).toBe("customize");
    expect(choice?.mode).toBe("custom");
    // The predicate the dashboard hides the curve editor behind.
    expect(isCustomizeMode(CUSTOMIZE_FAN_MODE)).toBe(true);
    expect(isCustomizeMode(0)).toBe(false);
    expect(isCustomizeMode(255)).toBe(false);
  });
});
