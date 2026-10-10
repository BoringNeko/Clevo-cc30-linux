import { beforeEach, describe, expect, it, vi } from "vitest";
import { getAutostart, setAutostart } from "./autostart";

const invoke = vi.fn();
vi.mock("./bridge", () => ({ invokeBridge: (...args: unknown[]) => invoke(...args) }));

describe("autostart API", () => {
  beforeEach(() => { invoke.mockReset(); });

  it("reads and writes the backend registration", async () => {
    invoke.mockResolvedValueOnce(false).mockResolvedValueOnce(true);
    expect(await getAutostart()).toBe(false);
    expect(invoke).toHaveBeenNthCalledWith(1, "get_autostart");
    expect(await setAutostart(true)).toBe(true);
    expect(invoke).toHaveBeenNthCalledWith(2, "set_autostart", { enabled: true });
  });

  it("propagates registration failures", async () => {
    invoke.mockRejectedValue(new Error("permission denied"));
    await expect(setAutostart(true)).rejects.toThrow("permission denied");
  });
});
