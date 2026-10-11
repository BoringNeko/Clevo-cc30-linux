import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { PowerPage } from "./PowerPage";
import type { PowerSnapshot } from "../api/power";

const getPower = vi.fn();
const setChargeThresholds = vi.fn();
const setPerfMode = vi.fn();
vi.mock("../api/power", async () => ({
  ...await vi.importActual<typeof import("../api/power")>("../api/power"),
  getPower: () => getPower(),
  setChargeThresholds: (...args: unknown[]) => setChargeThresholds(...args),
}));
vi.mock("../api/daemon", () => ({ setPerfMode: (...args: unknown[]) => setPerfMode(...args) }));

function snapshot(): PowerSnapshot {
  return {
    available: true, ac_online: true, errors: [],
    platform_profile: { driver: "clevo-cc", current: "balanced", choices: ["quiet", "low-power", "balanced", "performance"], reason: null },
    batteries: [{ name: "BAT0", present: true, capacity: 67, status: "Charging", health: "Good", health_percent: 90, cycle_count: 123, thresholds: { start: 40, end: 80 }, start_supported: true, end_supported: true, errors: [] }],
  };
}

beforeEach(() => {
  getPower.mockReset().mockResolvedValue(snapshot());
  setChargeThresholds.mockReset().mockResolvedValue({ start: 50, end: 90 });
  setPerfMode.mockReset().mockResolvedValue(2);
});

describe("PowerPage", () => {
  it("shows native discrete values and saves an explicit enable state", async () => {
    const data = snapshot();
    data.batteries[0].charge_control = { backend: "clevo-flexicharger", start_values: [40, 50, 60, 70, 80, 95], end_values: [60, 70, 80, 90, 100] };
    data.batteries[0].thresholds = { enabled: false, start: 40, end: 80 };
    getPower.mockResolvedValue(data);
    render(<PowerPage onModeChanged={() => {}} />);
    const toggle = await screen.findByRole("switch", { name: "充电阈值控制" });
    expect(toggle).not.toBeChecked();
    expect(screen.getByRole("combobox", { name: "开始充电 (%)" })).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(toggle);
    fireEvent.mouseDown(screen.getByRole("combobox", { name: "开始充电 (%)" }));
    expect(await screen.findByRole("option", { name: "95%" })).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: "41%" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("option", { name: "50%" }));
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(setChargeThresholds).toHaveBeenCalledWith("BAT0", 50, 80, true));
  });

  it("disables native charging control while preserving the stored window", async () => {
    const data = snapshot();
    data.batteries[0].charge_control = { backend: "clevo-flexicharger", start_values: [40, 50], end_values: [80, 90] };
    data.batteries[0].thresholds = { enabled: true, start: 40, end: 80 };
    getPower.mockResolvedValue(data);
    render(<PowerPage onModeChanged={() => {}} />);
    fireEvent.click(await screen.findByRole("switch", { name: "充电阈值控制" }));
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(setChargeThresholds).toHaveBeenCalledWith("BAT0", 40, 80, false));
  });

  it("shows actual battery values and desktop mode without writing on mount", async () => {
    render(<PowerPage onModeChanged={() => {}} />);
    expect(await screen.findByText("67%")).toBeInTheDocument();
    expect(screen.getByText("充电中")).toBeInTheDocument();
    expect(screen.getByText("健康度 90.0%")).toBeInTheDocument();
    expect(screen.getByText("循环次数 123")).toBeInTheDocument();
    expect(setChargeThresholds).not.toHaveBeenCalled();
    expect(setPerfMode).not.toHaveBeenCalled();
  });

  it("keeps unknown values unknown and disables unsupported charge controls", async () => {
    const data = snapshot();
    Object.assign(data.batteries[0], { capacity: null, health: null, health_percent: null, cycle_count: null, thresholds: null, start_supported: false, end_supported: false });
    data.platform_profile = { driver: null, current: null, choices: [], reason: "unsupported" };
    getPower.mockResolvedValue(data);
    render(<PowerPage onModeChanged={() => {}} />);
    expect(await screen.findByText("当前 Linux 驱动未提供充电阈值接口")).toBeInTheDocument();
    expect(screen.getByRole("spinbutton", { name: "停止充电 (%)" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "保存" })).toBeDisabled();
    expect(screen.getByText("桌面电源模式接口不可用")).toBeInTheDocument();
    expect(screen.queryByText("0%")).not.toBeInTheDocument();
  });

  it("validates ordering and sends only an explicit supported charge change", async () => {
    render(<PowerPage onModeChanged={() => {}} />);
    const start = await screen.findByRole("spinbutton", { name: "开始充电 (%)" });
    const end = screen.getByRole("spinbutton", { name: "停止充电 (%)" });
    fireEvent.change(start, { target: { value: "90" } });
    expect(screen.getByRole("button", { name: "保存" })).toBeDisabled();
    fireEvent.change(start, { target: { value: "50" } });
    fireEvent.change(end, { target: { value: "90" } });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(setChargeThresholds).toHaveBeenCalledWith("BAT0", 50, 90));
  });

  it("preserves denial errors and permits retry", async () => {
    setChargeThresholds.mockRejectedValue("AccessDenied");
    render(<PowerPage onModeChanged={() => {}} />);
    const end = await screen.findByRole("spinbutton", { name: "停止充电 (%)" });
    fireEvent.change(end, { target: { value: "90" } });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    expect(await screen.findByText("AccessDenied")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "保存" })).toBeEnabled();
  });

  it("uses the existing performance API for an advertised desktop profile", async () => {
    const changed = vi.fn();
    render(<PowerPage onModeChanged={changed} />);
    fireEvent.mouseDown(await screen.findByRole("combobox", { name: "电源模式" }));
    fireEvent.click(await screen.findByRole("option", { name: "高性能" }));
    await waitFor(() => expect(setPerfMode).toHaveBeenCalledWith("performance"));
    await waitFor(() => expect(changed).toHaveBeenCalled());
  });

  it("does not control desktop profiles owned by another driver", async () => {
    const data = snapshot();
    data.platform_profile.driver = null;
    getPower.mockResolvedValue(data);
    render(<PowerPage onModeChanged={() => {}} />);
    expect(await screen.findByText("桌面电源模式由其他驱动管理")).toBeInTheDocument();
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
  });

  it("shows daemon failures and refreshes after retry", async () => {
    getPower.mockRejectedValueOnce("daemon unavailable");
    render(<PowerPage onModeChanged={() => {}} />);
    expect(await screen.findByText(/daemon unavailable/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "刷新电源信息" }));
    expect(await screen.findByText("67%")).toBeInTheDocument();
  });
});
