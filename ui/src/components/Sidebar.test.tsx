import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { NAV, Sidebar } from "./Sidebar";
import { FALLBACK_PALETTE } from "../lib/color";
import { DEFAULT_APPEARANCE } from "../theme";

function setup() {
  const onNavigate = vi.fn();
  render(
    <Sidebar
      palette={FALLBACK_PALETTE}
      active="overview"
      blur
      appearance={DEFAULT_APPEARANCE}
      logo={null}
      onNavigate={onNavigate}
      onOpenSettings={() => {}}
    />,
  );
  return { onNavigate };
}

describe("Sidebar", () => {
  it("offers an RGB page alongside overview and fans", () => {
    expect(NAV.map((item) => item.id)).toEqual(["overview", "fans", "rgb"]);
  });

  it("renders a navigable RGB entry", async () => {
    const user = userEvent.setup();
    const { onNavigate } = setup();
    await user.click(screen.getByRole("button", { name: "RGB 灯效" }));
    expect(onNavigate).toHaveBeenCalledWith("rgb");
  });

  it("marks the active page with aria-current", () => {
    setup();
    expect(screen.getByRole("button", { name: "概览" }).getAttribute("aria-current")).toBe("true");
    expect(screen.getByRole("button", { name: "RGB 灯效" }).getAttribute("aria-current")).toBeNull();
  });
});
