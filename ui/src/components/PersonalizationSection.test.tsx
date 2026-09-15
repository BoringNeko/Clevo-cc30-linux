import { describe, expect, it } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { PersonalizationSection } from "./PersonalizationSection";
import { FALLBACK_PALETTE } from "../lib/color";
import { DEFAULT_APPEARANCE, type Appearance } from "../theme";

function setup(
  appearance: Appearance,
  onAppearanceChange: (patch: Partial<Appearance>) => void = () => {},
) {
  render(
    <PersonalizationSection
      palette={FALLBACK_PALETTE}
      onWallpaperChange={() => {}}
      onResetWallpaper={() => {}}
      wallpaperIsCustom={false}
      appearance={appearance}
      onAppearanceChange={onAppearanceChange}
      logo={null}
      logoIsCustom={false}
      onLogoChange={() => {}}
      onLogoReset={() => {}}
    />,
  );
}

describe("PersonalizationSection", () => {
  it("offers dark and light modes and reports the choice", () => {
    const changes: Partial<Appearance>[] = [];
    setup(DEFAULT_APPEARANCE, (p: Partial<Appearance>) => changes.push(p),
    );
    fireEvent.click(screen.getByRole("button", { name: /浅色/ }));
    expect(changes).toContainEqual({ mode: "light" });
  });

  it("opens the custom picker for the accent colour", () => {
    setup(DEFAULT_APPEARANCE);
    // One trigger per colour setting (accent, surface, text), each labelled.
    expect(screen.getByRole("button", { name: "强调色" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "卡片颜色" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "文字颜色" })).toBeTruthy();
    // No native colour input remains.
    expect(document.querySelectorAll('input[type="color"]').length).toBe(0);
  });

  it("reports a custom accent typed in the picker", () => {
    const changes: Partial<Appearance>[] = [];
    setup(DEFAULT_APPEARANCE, (p: Partial<Appearance>) => changes.push(p),
    );
    fireEvent.click(screen.getByRole("button", { name: "强调色" }));
    fireEvent.change(screen.getByLabelText("十六进制颜色"), { target: { value: "#123456" } });
    expect(changes).toContainEqual({ accent: "#123456" });
  });

  it("resets a colour to default", () => {
    const changes: Partial<Appearance>[] = [];
    setup({ ...DEFAULT_APPEARANCE, accent: "#123456" }, (p: Partial<Appearance>) => changes.push(p),
    );
    // The first enabled 默认 button resets the accent.
    const resetButtons = screen.getAllByRole("button", { name: "默认" });
    fireEvent.click(resetButtons[0]);
    expect(changes).toContainEqual({ accent: null });
  });
});
