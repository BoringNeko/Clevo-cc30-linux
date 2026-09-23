import { describe, expect, it, beforeEach, vi } from "vitest";
import { renderHook, waitFor, act } from "@testing-library/react";

const invoke = vi.hoisted(() => vi.fn());
// The hooks now dispatch through the shell-agnostic bridge; mocking it keeps
// these tests independent of which shell (Tauri or Electron) would host them.
vi.mock("../api/bridge", () => ({ invokeBridge: invoke }));

import { useWallpaper } from "./useWallpaper";
import { FALLBACK_PALETTE } from "../lib/color";

const DEFAULT_WALLPAPER = "/wallpaper-default.svg";

function imageFile(name = "my.png", type = "image/png") {
  return new File([new Uint8Array([1, 2, 3])], name, { type });
}

describe("useWallpaper", () => {
  beforeEach(() => {
    invoke.mockReset();
    document.documentElement.removeAttribute("style");
  });

  it("restores the saved wallpaper as a data URL on startup", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "load_wallpaper") return "data:image/png;base64,SAVED";
      return null;
    });

    const { result } = renderHook(() => useWallpaper());

    await waitFor(() => expect(result.current.wallpaper).toBe("data:image/png;base64,SAVED"));
    expect(result.current.isCustom).toBe(true);
    expect(invoke).toHaveBeenCalledWith("load_wallpaper");
  });

  it("falls back to the default wallpaper when nothing is saved", async () => {
    invoke.mockResolvedValue(null);

    const { result } = renderHook(() => useWallpaper());

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("load_wallpaper"));
    expect(result.current.wallpaper).toBe(DEFAULT_WALLPAPER);
    expect(result.current.isCustom).toBe(false);
  });

  it("persists a chosen wallpaper via the backend and marks it custom", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "load_wallpaper") return null;
      if (cmd === "save_wallpaper") return { data_url: "data:image/png;base64,NEW" };
      return null;
    });

    const { result } = renderHook(() => useWallpaper());
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("load_wallpaper"));

    await act(async () => {
      await result.current.setWallpaperFromFile(imageFile());
    });

    await waitFor(() => expect(result.current.wallpaper).toBe("data:image/png;base64,NEW"));
    expect(result.current.isCustom).toBe(true);
    expect(invoke).toHaveBeenCalledWith(
      "save_wallpaper",
      expect.objectContaining({ ext: "png", dataBase64: expect.any(String) }),
    );
  });

  it("ignores files that are not images", async () => {
    invoke.mockResolvedValue(null);

    const { result } = renderHook(() => useWallpaper());
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("load_wallpaper"));

    await act(async () => {
      await result.current.setWallpaperFromFile(new File(["x"], "notes.txt", { type: "text/plain" }));
    });

    expect(invoke).not.toHaveBeenCalledWith("save_wallpaper", expect.anything());
    expect(result.current.isCustom).toBe(false);
  });

  it("clears the saved wallpaper on reset", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "load_wallpaper") return "data:image/png;base64,SAVED";
      if (cmd === "clear_wallpaper") return null;
      return null;
    });

    const { result } = renderHook(() => useWallpaper());
    await waitFor(() => expect(result.current.isCustom).toBe(true));

    await act(async () => {
      await result.current.resetWallpaper();
    });

    expect(invoke).toHaveBeenCalledWith("clear_wallpaper");
    await waitFor(() => expect(result.current.wallpaper).toBe(DEFAULT_WALLPAPER));
    expect(result.current.isCustom).toBe(false);
  });

  it("exposes a palette and publishes it to CSS variables", async () => {
    // jsdom never loads images; make `src` assignment fire onerror so the
    // fallback palette flows through applyPalette into the CSS variables.
    const OriginalImage = globalThis.Image;
    class FakeImage {
      onload: (() => void) | null = null;
      onerror: (() => void) | null = null;
      crossOrigin: string | null = null;
      set src(_value: string) {
        this.onerror?.();
      }
    }
    // @ts-expect-error test stub
    globalThis.Image = FakeImage;
    invoke.mockResolvedValue(null);

    try {
      const { result } = renderHook(() => useWallpaper());

      await waitFor(() => expect(result.current.palette).toEqual(FALLBACK_PALETTE));
      await waitFor(() =>
        expect(document.documentElement.style.getPropertyValue("--accent")).not.toBe(""),
      );
    } finally {
      globalThis.Image = OriginalImage;
    }
  });
});
