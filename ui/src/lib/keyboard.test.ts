import { describe, expect, it } from "vitest";
import {
  applyZone,
  isSingleZone,
  keyboardBackendLabel,
  keyboardModeLabel,
  keyboardSupportedModes,
  keyboardUnavailableMessage,
  keyboardWritable,
  keyboardZoneLabel,
  keysColorToHex,
  normaliseKeys,
} from "./keyboard";
import type { KeyboardState } from "../api/daemon";

function state(patch: Partial<KeyboardState> = {}): KeyboardState {
  return {
    available: true,
    writable: true,
    backend: "usb-hid",
    mode: "static",
    brightness: 75,
    keys: normaliseKeys(undefined),
    ...patch,
  };
}

describe("keyboard helpers", () => {
  it("labels every mode and zone", () => {
    expect(keyboardModeLabel("off")).toBe("关闭");
    expect(keyboardModeLabel("static")).toBe("静态");
    expect(keyboardZoneLabel("all")).toBe("全部");
    expect(keyboardZoneLabel("right")).toBe("右");
  });

  it("treats only an available and writable controller as editable", () => {
    expect(keyboardWritable(state())).toBe(true);
    expect(keyboardWritable(state({ writable: false }))).toBe(false);
    expect(keyboardWritable(state({ available: false }))).toBe(false);
    expect(keyboardWritable(null)).toBe(false);
  });

  it("always returns a 6x20 grid, padding a missing report", () => {
    const grid = normaliseKeys(undefined);
    expect(grid).toHaveLength(6);
    expect(grid[0]).toHaveLength(20);
    // A malformed report is replaced rather than trusted.
    expect(normaliseKeys([[1, 2]] as unknown as number[][][])).toHaveLength(6);
  });

  it("applies a colour to exactly the requested zone", () => {
    const red: [number, number, number] = [255, 0, 0];
    const left = applyZone(normaliseKeys(undefined), "left", red);
    expect(left[0][0]).toEqual(red);
    expect(left[0][6]).toEqual(red);
    expect(left[0][7]).toEqual([0, 0, 0]);

    const middle = applyZone(normaliseKeys(undefined), "middle", red);
    expect(middle[0][7]).toEqual(red);
    expect(middle[0][12]).toEqual(red);
    expect(middle[0][13]).toEqual([0, 0, 0]);

    const right = applyZone(normaliseKeys(undefined), "right", red);
    expect(right[0][13]).toEqual(red);
    expect(right[0][19]).toEqual(red);
    expect(right[0][12]).toEqual([0, 0, 0]);

    const all = applyZone(normaliseKeys(undefined), "all", red);
    expect(all[5][19]).toEqual(red);
  });

  it("does not mutate the grid it is given", () => {
    const original = normaliseKeys(undefined);
    applyZone(original, "all", [1, 2, 3]);
    expect(original[0][0]).toEqual([0, 0, 0]);
  });

  it("keeps the static mode list stable", () => {
    const reported = keyboardSupportedModes(state({ modes: ["static", "off"] }));
    expect(reported).toEqual(["off", "static"]);
  });

  it("recognises the single-zone backend", () => {
    expect(isSingleZone(state({ backend: "acpi-dchu" }))).toBe(true);
    expect(isSingleZone(state({ backend: "usb-hid" }))).toBe(false);
  });

  it("names the detected backend", () => {
    expect(keyboardBackendLabel(state({ backend: "acpi-dchu" }))).toContain("单区");
    expect(keyboardBackendLabel(state({ backend: "usb-hid" }))).toContain("USB HID");
  });

  it("explains a firmware capability with no Linux channel", () => {
    expect(keyboardUnavailableMessage(state({ available: false, firmware_kb_type: 6 }))).toContain(
      "没有可验证的 Linux 写入通道",
    );
    expect(keyboardUnavailableMessage(state({ available: false }))).toContain(
      "未检测到可写的键盘灯控制器",
    );
    expect(keyboardUnavailableMessage(null)).toContain("未检测到");
  });

  it("formats a key colour as hex and falls back to black", () => {
    expect(keysColorToHex([255, 0, 128])).toBe("#ff0080");
    expect(keysColorToHex(undefined)).toBe("#000000");
    expect(keysColorToHex([1, 2])).toBe("#000000");
  });
});
