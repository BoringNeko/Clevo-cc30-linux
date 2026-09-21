import { describe, expect, it, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { DisplaySection } from "./DisplaySection";
import { FALLBACK_PALETTE } from "../lib/color";
import { DEFAULT_APPEARANCE, type Appearance } from "../theme";

function setup(
  appearance: Appearance = DEFAULT_APPEARANCE,
  onAppearanceChange: (patch: Partial<Appearance>) => void = () => {},
  onResize: (w: number, h: number) => void = () => {},
) {
  render(
    <DisplaySection
      palette={FALLBACK_PALETTE}
      appearance={appearance}
      onAppearanceChange={onAppearanceChange}
      onResize={onResize}
    />,
  );
}

describe("DisplaySection", () => {
  it("defaults to 16:9 with a 16:9 resolution", () => {
    setup();
    expect((screen.getByRole("combobox", { name: "分辨率" }) as HTMLElement).textContent).toContain(
      "1600 × 900",
    );
    expect(screen.getByDisplayValue("100")).toBeTruthy();
  });

  it("offers both aspect ratios", () => {
    setup();
    expect(screen.getByRole("button", { name: "16:9" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "16:10" })).toBeTruthy();
  });

  it("snaps to the ratio's default resolution when the ratio changes", async () => {
    const user = userEvent.setup();
    const changes: Partial<Appearance>[] = [];
    const resize = vi.fn();
    setup(DEFAULT_APPEARANCE, (p) => changes.push(p), resize);
    await user.click(screen.getByRole("button", { name: "16:10" }));
    // 16:10 has no 1600x900, so it falls back to the preset closest to the
    // design width (1680x1050).
    expect(changes).toContainEqual({ aspect: "16:10", displayWidth: 1680, displayHeight: 1050 });
    expect(resize).toHaveBeenCalledWith(1680, 1050);
  });

  it("shows only the resolutions matching the current ratio", async () => {
    const user = userEvent.setup();
    setup({ ...DEFAULT_APPEARANCE, aspect: "16:10", displayWidth: 1680, displayHeight: 1050 });
    await user.click(screen.getByRole("combobox", { name: "分辨率" }));
    const options = screen.getAllByRole("option").map((o) => o.textContent);
    expect(options.some((t) => t?.includes("1920 × 1200"))).toBe(true);
    // A 16:9-only size is not offered.
    expect(options.some((t) => t?.includes("1920 × 1080"))).toBe(false);
  });

  it("reports the chosen resolution and resizes", async () => {
    const user = userEvent.setup();
    const changes: Partial<Appearance>[] = [];
    const resize = vi.fn();
    setup(DEFAULT_APPEARANCE, (p) => changes.push(p), resize);
    await user.click(screen.getByRole("combobox", { name: "分辨率" }));
    await user.click(screen.getByRole("option", { name: /3840 × 2160/ }));
    expect(changes).toContainEqual({ displayWidth: 3840, displayHeight: 2160 });
    expect(resize).toHaveBeenCalledWith(3840, 2160);
  });

  it("commits a typed scale percentage on blur", () => {
    const changes: Partial<Appearance>[] = [];
    setup({ ...DEFAULT_APPEARANCE, scale: 100 }, (p) => changes.push(p));
    const input = screen.getByLabelText("缩放百分比");
    fireEvent.change(input, { target: { value: "150" } });
    fireEvent.blur(input);
    expect(changes).toContainEqual({ scale: 150 });
  });

  it("clamps a typed scale to the allowed range", () => {
    const changes: Partial<Appearance>[] = [];
    setup({ ...DEFAULT_APPEARANCE, scale: 100 }, (p) => changes.push(p));
    const input = screen.getByLabelText("缩放百分比");
    fireEvent.change(input, { target: { value: "900" } });
    fireEvent.blur(input);
    expect(changes).toContainEqual({ scale: 200 });
  });

  it("resets display settings to defaults", async () => {
    const user = userEvent.setup();
    const changes: Partial<Appearance>[] = [];
    const resize = vi.fn();
    setup(
      { ...DEFAULT_APPEARANCE, aspect: "16:10", displayWidth: 3840, displayHeight: 2400, scale: 200 },
      (p) => changes.push(p),
      resize,
    );
    await user.click(screen.getByRole("button", { name: "恢复显示默认设置" }));
    expect(changes).toContainEqual({
      aspect: "16:9",
      displayWidth: 1600,
      displayHeight: 900,
      scale: 100,
    });
    expect(resize).toHaveBeenCalledWith(1600, 900);
  });
});
