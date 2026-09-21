import { describe, expect, it, beforeEach } from "vitest";
import {
  readAppearance,
  readBlurSetting,
  writeAppearance,
  writeBlurSetting,
  readCompatibilityPrefs,
  writeCompatibilityPrefs,
} from "./useAppSettings";
import { DEFAULT_APPEARANCE } from "../theme";

describe("app settings", () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it("defaults blur to auto", () => {
    expect(readBlurSetting()).toBe("auto");
  });

  it("persists the blur setting", () => {
    writeBlurSetting("off");
    expect(readBlurSetting()).toBe("off");
    writeBlurSetting("on");
    expect(readBlurSetting()).toBe("on");
  });

  it("ignores an invalid stored blur value", () => {
    window.localStorage.setItem("clevo.blur", "bogus");
    expect(readBlurSetting()).toBe("auto");
  });

  it("defaults compatibility to auto + hardware rendering", () => {
    expect(readCompatibilityPrefs()).toEqual({
      backend: "auto",
      softwareRendering: false,
    });
  });

  it("ignores an invalid stored backend", () => {
    window.localStorage.setItem("clevo.backend", "bogus");
    expect(readCompatibilityPrefs().backend).toBe("auto");
  });

  it("persists compatibility prefs", () => {
    writeCompatibilityPrefs({ backend: "x11", softwareRendering: true });
    expect(readCompatibilityPrefs()).toEqual({
      backend: "x11",
      softwareRendering: true,
    });
  });

  it("defaults display settings", () => {
    const a = readAppearance();
    expect(a.aspect).toBe("16:9");
    // The UI is designed at 1600x900, so that is the default window size.
    expect(a.displayWidth).toBe(1600);
    expect(a.displayHeight).toBe(900);
    expect(a.scale).toBe(100);
  });

  it("round-trips display settings", () => {
    writeAppearance({
      ...DEFAULT_APPEARANCE,
      aspect: "16:10",
      displayWidth: 1920,
      displayHeight: 1200,
      scale: 150,
    });
    const a = readAppearance();
    expect(a.aspect).toBe("16:10");
    expect(a.displayWidth).toBe(1920);
    expect(a.displayHeight).toBe(1200);
    expect(a.scale).toBe(150);
  });

  it("clamps the scale and snaps an unknown resolution to the ratio default", () => {
    window.localStorage.setItem(
      "clevo.appearance",
      JSON.stringify({ aspect: "16:10", displayWidth: 9999, displayHeight: 9999, scale: 900 }),
    );
    const a = readAppearance();
    expect(a.aspect).toBe("16:10");
    // Falls back to the 16:10 preset closest to the 1600x900 design size.
    expect(a.displayWidth).toBe(1680);
    expect(a.displayHeight).toBe(1050);
    expect(a.scale).toBe(200);
  });
});
