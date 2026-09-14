import { describe, expect, it, beforeEach } from "vitest";
import {
  readBlurSetting,
  writeBlurSetting,
  readCompatibilityPrefs,
  writeCompatibilityPrefs,
} from "./useAppSettings";

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
});
