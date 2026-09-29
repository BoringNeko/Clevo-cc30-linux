import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SettingsDialog } from "./SettingsDialog";
import { FALLBACK_PALETTE } from "../lib/color";
import { DEFAULT_APPEARANCE } from "../theme";

const getKeyboard = vi.fn();

vi.mock("../api/daemon", async () => {
  const actual = await vi.importActual<typeof import("../api/daemon")>("../api/daemon");
  return {
    ...actual,
    getKeyboard: () => getKeyboard(),
  };
});

function renderDialog() {
  return render(
    <SettingsDialog
      open
      onClose={() => {}}
      palette={FALLBACK_PALETTE}
      onWallpaperChange={() => {}}
      onResetWallpaper={() => {}}
      wallpaperIsCustom={false}
      blurSetting="auto"
      onBlurSettingChange={() => {}}
      appearance={DEFAULT_APPEARANCE}
      onAppearanceChange={() => {}}
      logo={null}
      logoIsCustom={false}
      onLogoChange={() => {}}
      onLogoReset={() => {}}
      compatibility={{ backend: "auto", softwareRendering: false }}
      onCompatibilityChange={() => {}}
      onResize={() => {}}
    />,
  );
}

describe("SettingsDialog", () => {
  it("shows the section navigation and personalization content", () => {
    renderDialog();
    expect(screen.getAllByText("个性化").length).toBeGreaterThan(0);
    expect(screen.getAllByText("显示").length).toBeGreaterThan(0);
    expect(screen.getAllByText("兼容性").length).toBeGreaterThan(0);
    expect(screen.getByText("自定义壁纸")).toBeTruthy();
  });

  it("shows the display section with resolution and scale", async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(screen.getByRole("button", { name: "显示" }));
    expect(screen.getAllByText("分辨率").length).toBeGreaterThan(0);
    expect(screen.getByText("缩放")).toBeTruthy();
  });

  it("switches to the compatibility section with explanations", async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(screen.getByRole("button", { name: "兼容性" }));
    expect(screen.getByText("显示后端")).toBeTruthy();
    expect(screen.getByText("软件渲染")).toBeTruthy();
    expect(screen.getByText("玻璃模糊")).toBeTruthy();
    // The consequence of changing the backend is stated.
    expect(
      screen.getByText(/需要重启应用后生效/),
    ).toBeTruthy();
  });

  it("opens the keyboard section and reports an unavailable controller", async () => {
    getKeyboard.mockResolvedValue({
      available: false,
      writable: false,
      backend: "none",
      mode: "off",
      brightness: 0,
      keys: [],
    });
    const user = userEvent.setup();
    renderDialog();
    await user.click(screen.getByRole("button", { name: "键盘灯" }));
    expect(await screen.findByText(/未检测到可写的键盘灯控制器/)).toBeTruthy();
  });
});
