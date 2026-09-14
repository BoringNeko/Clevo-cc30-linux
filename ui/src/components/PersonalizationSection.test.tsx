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

  it("reports a custom accent from the colour input", () => {
    const changes: Partial<Appearance>[] = [];
    setup(DEFAULT_APPEARANCE, (p: Partial<Appearance>) => changes.push(p),
    );
    const inputs = document.querySelectorAll('input[type="color"]');
    expect(inputs.length).toBeGreaterThan(0);
    fireEvent.change(inputs[0], { target: { value: "#123456" } });
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
