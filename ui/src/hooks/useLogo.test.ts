import { describe, expect, it, beforeEach, vi } from "vitest";
import { renderHook, waitFor, act } from "@testing-library/react";

const invoke = vi.hoisted(() => vi.fn());
// The hooks now dispatch through the shell-agnostic bridge; mocking it keeps
// these tests independent of which shell (Tauri or Electron) would host them.
vi.mock("../api/bridge", () => ({ invokeBridge: invoke }));

import { useLogo } from "./useLogo";

const DEFAULT_PATH = "logo.jpg";

function imageFile(name = "custom.png", type = "image/png") {
  return new File([new Uint8Array([1, 2, 3])], name, { type });
}

describe("useLogo", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it("uses an app-relative default asset directly, without the backend", async () => {
    invoke.mockResolvedValue(null);

    const { result } = renderHook(() => useLogo(DEFAULT_PATH));

    await waitFor(() => expect(result.current.logo).toBe(DEFAULT_PATH));
    expect(result.current.isCustom).toBe(false);
    expect(invoke).not.toHaveBeenCalledWith("read_image_path", expect.anything());
  });

  it("reads an absolute default path from the backend as a data URL", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "load_logo") return null;
      if (cmd === "read_image_path") return "data:image/jpeg;base64,AAAA";
      return null;
    });

    const { result } = renderHook(() => useLogo("/home/user/pic/logo.jpg"));

    await waitFor(() => expect(result.current.logo).toBe("data:image/jpeg;base64,AAAA"));
    expect(result.current.isCustom).toBe(false);
    expect(invoke).toHaveBeenCalledWith("read_image_path", { path: "/home/user/pic/logo.jpg" });
  });

  it("prefers a saved custom logo over the default path", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "load_logo") return "data:image/png;base64,SAVED";
      return null;
    });

    const { result } = renderHook(() => useLogo(DEFAULT_PATH));

    await waitFor(() => expect(result.current.logo).toBe("data:image/png;base64,SAVED"));
    expect(result.current.isCustom).toBe(true);
  });

  it("has no logo when the default path cannot be read", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "load_logo") return null;
      if (cmd === "read_image_path") return null;
      return null;
    });

    const { result } = renderHook(() => useLogo("/home/user/pic/logo.jpg"));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("read_image_path", { path: "/home/user/pic/logo.jpg" }));
    expect(result.current.logo).toBeNull();
    expect(result.current.isCustom).toBe(false);
  });

  it("does not read a default path when none is configured", async () => {
    invoke.mockResolvedValue(null);

    const { result } = renderHook(() => useLogo(""));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("load_logo"));
    expect(result.current.logo).toBeNull();
    expect(invoke).not.toHaveBeenCalledWith("read_image_path", expect.anything());
  });

  it("saves a chosen image and marks it custom", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "load_logo") return null;
      if (cmd === "save_logo") return { data_url: "data:image/png;base64,NEW" };
      return null;
    });

    const { result } = renderHook(() => useLogo(DEFAULT_PATH));
    await waitFor(() => expect(result.current.logo).toBe(DEFAULT_PATH));

    await act(async () => {
      await result.current.setLogoFromFile(imageFile());
    });

    await waitFor(() => expect(result.current.logo).toBe("data:image/png;base64,NEW"));
    expect(result.current.isCustom).toBe(true);
    expect(invoke).toHaveBeenCalledWith(
      "save_logo",
      expect.objectContaining({ ext: "png", dataBase64: expect.any(String) }),
    );
  });

  it("ignores files that are not images", async () => {
    invoke.mockResolvedValue(null);

    const { result } = renderHook(() => useLogo(DEFAULT_PATH));
    await waitFor(() => expect(result.current.logo).toBe(DEFAULT_PATH));

    await act(async () => {
      await result.current.setLogoFromFile(new File(["x"], "notes.txt", { type: "text/plain" }));
    });

    expect(invoke).not.toHaveBeenCalledWith("save_logo", expect.anything());
    expect(result.current.logo).toBe(DEFAULT_PATH);
    expect(result.current.isCustom).toBe(false);
  });

  it("resets back to the default image", async () => {
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "load_logo") return "data:image/png;base64,SAVED";
      if (cmd === "clear_logo") return null;
      return null;
    });

    const { result } = renderHook(() => useLogo(DEFAULT_PATH));
    await waitFor(() => expect(result.current.isCustom).toBe(true));

    await act(async () => {
      await result.current.resetLogo();
    });

    expect(invoke).toHaveBeenCalledWith("clear_logo");
    await waitFor(() => expect(result.current.logo).toBe(DEFAULT_PATH));
    expect(result.current.isCustom).toBe(false);
  });
});
