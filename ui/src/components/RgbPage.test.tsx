import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { RgbPage } from "./RgbPage";
import { FALLBACK_PALETTE } from "../lib/color";
import type { KeyboardState } from "../api/daemon";

const getKeyboard = vi.fn();
const setKeyboardMode = vi.fn();
const setKeyboardBrightness = vi.fn();
const setKeyboardZone = vi.fn();

vi.mock("../api/daemon", async () => {
  const actual = await vi.importActual<typeof import("../api/daemon")>("../api/daemon");
  return {
    ...actual,
    getKeyboard: () => getKeyboard(),
    setKeyboardMode: (mode: string) => setKeyboardMode(mode),
    setKeyboardBrightness: (level: number) => setKeyboardBrightness(level),
    setKeyboardZone: (zone: string, color: number[]) => setKeyboardZone(zone, color),
  };
});

function keyboardState(patch: Partial<KeyboardState> = {}): KeyboardState {
  return {
    available: true,
    writable: true,
    backend: "acpi-dchu",
    firmware_kb_type: 6,
    mode: "static",
    brightness: 75,
    // What the real single-zone RGB15 backend reports.
    modes: ["off", "static"],
    keys: Array.from({ length: 6 }, () => Array.from({ length: 20 }, () => [255, 0, 0])),
    ...patch,
  };
}

describe("RgbPage", () => {
  beforeEach(() => {
    getKeyboard.mockReset();
    setKeyboardMode.mockReset().mockResolvedValue(undefined);
    setKeyboardBrightness.mockReset().mockResolvedValue(75);
    setKeyboardZone.mockReset().mockResolvedValue(undefined);
  });

  it("shows the colour controls for a writable single-zone controller", async () => {
    getKeyboard.mockResolvedValue(keyboardState());
    render(<RgbPage palette={FALLBACK_PALETTE} />);

    await waitFor(() => expect(screen.getByText("键盘灯效")).toBeTruthy());
    expect(screen.getByText("亮度")).toBeTruthy();
    // A single-zone machine states there is one channel rather than showing zones.
    expect(screen.getByText(/整块键盘共用一个颜色通道/)).toBeTruthy();
    expect(screen.queryByDisplayValue("left")).toBeNull();
  });

  it("reports the detected backend and firmware type", async () => {
    getKeyboard.mockResolvedValue(keyboardState());
    render(<RgbPage palette={FALLBACK_PALETTE} />);

    await waitFor(() => expect(screen.getByText("ACPI-DCHU RGB15 · 单区")).toBeTruthy());
    // `6` now also appears on the keyboard preview, so scope to the value row.
    expect(screen.getAllByText("6").length).toBeGreaterThan(0);
    expect(screen.getByText("是")).toBeTruthy();
  });

  it("offers exactly the effects the controller reports", async () => {
    // A controller that reports more drives more.
    getKeyboard.mockResolvedValue(
      keyboardState({ modes: ["off", "static", "breath", "cycle", "wave", "dance", "tempo", "flash", "random"] }),
    );
    render(<RgbPage palette={FALLBACK_PALETTE} />);

    await waitFor(() => expect(screen.getByText("键盘灯效")).toBeTruthy());
    for (const label of ["关闭", "静态", "呼吸", "循环", "波浪", "舞动", "节奏", "闪烁", "随机"]) {
      expect(screen.getByRole("button", { name: label })).toBeTruthy();
    }
  });

  it("shows only off/static for the single-zone RGB15 controller", async () => {
    // The real COLORFUL P15 23 backend reports exactly these two: the EC accepts
    // the vendor effect words but never animates for them.
    getKeyboard.mockResolvedValue(keyboardState({ modes: ["off", "static"] }));
    render(<RgbPage palette={FALLBACK_PALETTE} />);

    await waitFor(() => expect(screen.getByText("键盘灯效")).toBeTruthy());
    expect(screen.getByRole("button", { name: "关闭" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "静态" })).toBeTruthy();
    for (const label of ["呼吸", "循环", "波浪", "舞动", "节奏", "闪烁", "随机"]) {
      expect(screen.queryByRole("button", { name: label })).toBeNull();
    }
  });

  it("does not offer effects the controller cannot drive", async () => {
    getKeyboard.mockResolvedValue(keyboardState({ modes: ["off", "static", "wave"] }));
    render(<RgbPage palette={FALLBACK_PALETTE} />);

    await waitFor(() => expect(screen.getByText("键盘灯效")).toBeTruthy());
    expect(screen.getByRole("button", { name: "波浪" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "呼吸" })).toBeNull();
    expect(screen.queryByRole("button", { name: "随机" })).toBeNull();
  });

  it("writes a native RGB15 effect through the daemon", async () => {
    getKeyboard.mockResolvedValue(
      keyboardState({ modes: ["off", "static", "breath", "cycle", "wave"] }),
    );
    render(<RgbPage palette={FALLBACK_PALETTE} />);

    fireEvent.click(await screen.findByRole("button", { name: "呼吸" }));
    await waitFor(() => expect(setKeyboardMode).toHaveBeenCalledWith("breath"));
  });

  it("shows the animated stage instead of a redundant grid for a single zone", async () => {
    getKeyboard.mockResolvedValue(keyboardState());
    render(<RgbPage palette={FALLBACK_PALETTE} />);

    // The studio preview is the colour readout on a single-zone board: a
    // second 6x20 grid would just repeat the same colour 120 times.
    await waitFor(() => expect(screen.getByText("整块键盘共用一个颜色通道")).toBeTruthy());
    expect(screen.queryByLabelText("键盘灯颜色预览")).toBeNull();
  });

  it("writes the chosen effect mode through the daemon", async () => {
    getKeyboard.mockResolvedValue(keyboardState({ modes: ["off", "static", "wave"] }));
    render(<RgbPage palette={FALLBACK_PALETTE} />);

    const wave = await screen.findByRole("button", { name: "波浪" });
    fireEvent.click(wave);
    await waitFor(() => expect(setKeyboardMode).toHaveBeenCalledWith("wave"));
  });

  it("applies the current colour to the single channel", async () => {
    getKeyboard.mockResolvedValue(keyboardState());
    render(<RgbPage palette={FALLBACK_PALETTE} />);

    fireEvent.click(await screen.findByRole("button", { name: "应用颜色" }));
    await waitFor(() =>
      expect(setKeyboardZone).toHaveBeenCalledWith("all", [255, 0, 0]),
    );
  });

  it("explains a firmware capability that has no Linux channel", async () => {
    getKeyboard.mockResolvedValue(
      keyboardState({ available: false, writable: false, backend: "none", firmware_kb_type: 6 }),
    );
    render(<RgbPage palette={FALLBACK_PALETTE} />);

    await waitFor(() =>
      expect(screen.getAllByText(/没有可验证的 Linux 写入通道/).length).toBeGreaterThan(0),
    );
    // No writable controls are offered.
    expect(screen.queryByRole("button", { name: "应用颜色" })).toBeNull();
  });

  it("surfaces a daemon error without pretending the write succeeded", async () => {
    getKeyboard.mockResolvedValue(keyboardState({ modes: ["off", "static", "wave"] }));
    setKeyboardMode.mockRejectedValue(new Error("权限不足"));
    render(<RgbPage palette={FALLBACK_PALETTE} />);

    fireEvent.click(await screen.findByRole("button", { name: "波浪" }));
    await waitFor(() => expect(screen.getByTestId("rgb-error")).toBeTruthy());
  });
});
