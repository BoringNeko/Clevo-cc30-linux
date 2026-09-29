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
    brightness: 3,
    keys: Array.from({ length: 6 }, () => Array.from({ length: 20 }, () => [255, 0, 0])),
    ...patch,
  };
}

describe("RgbPage", () => {
  beforeEach(() => {
    getKeyboard.mockReset();
    setKeyboardMode.mockReset().mockResolvedValue(undefined);
    setKeyboardBrightness.mockReset().mockResolvedValue(3);
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
    expect(screen.getByText("6")).toBeTruthy();
    expect(screen.getByText("是")).toBeTruthy();
  });

  it("writes the chosen effect mode through the daemon", async () => {
    getKeyboard.mockResolvedValue(keyboardState());
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
    getKeyboard.mockResolvedValue(keyboardState());
    setKeyboardMode.mockRejectedValue(new Error("权限不足"));
    render(<RgbPage palette={FALLBACK_PALETTE} />);

    fireEvent.click(await screen.findByRole("button", { name: "波浪" }));
    await waitFor(() => expect(screen.getByTestId("rgb-error")).toBeTruthy());
  });
});
