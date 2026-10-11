import { describe, expect, it, vi } from "vitest";
import { getPower, setChargeThresholds } from "./power";
const invoke = vi.fn();
vi.mock("./bridge", () => ({ invokeBridge: (...args: unknown[]) => invoke(...args) }));
describe("power bridge", () => {
  it("dispatches power reads and charge writes with explicit nullable start", async () => {
    invoke.mockResolvedValue({ available: true });
    await getPower();
    expect(invoke).toHaveBeenCalledWith("get_power");
    await setChargeThresholds("BAT0", null, 80);
    expect(invoke).toHaveBeenCalledWith("set_charge_thresholds", { battery: "BAT0", start: null, end: 80 });
    await setChargeThresholds("BAT0", 40, 80, false);
    expect(invoke).toHaveBeenCalledWith("set_charge_thresholds", { battery: "BAT0", start: 40, end: 80, enabled: false });
  });
});
