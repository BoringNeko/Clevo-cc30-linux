import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { ApplicationSection } from "./ApplicationSection";

const quitApp = vi.fn();
const getAutostart = vi.fn();
const setAutostart = vi.fn();

vi.mock("../api/autostart", () => ({
  getAutostart: () => getAutostart(),
  setAutostart: (enabled: boolean) => setAutostart(enabled),
}));

vi.mock("../api/daemon", () => ({
  quitApp: () => quitApp(),
}));

describe("ApplicationSection", () => {
  beforeEach(() => {
    quitApp.mockReset().mockResolvedValue(undefined);
    getAutostart.mockReset().mockResolvedValue(false);
    setAutostart.mockReset().mockImplementation(async (enabled: boolean) => enabled);
  });

  it("explains that closing hides to the tray", async () => {
    render(<ApplicationSection />);
    expect(screen.getByText("关闭窗口的行为")).toBeTruthy();
    expect(screen.getAllByText(/系统托盘/).length).toBeGreaterThan(0);
    await waitFor(() => expect(screen.getByRole("switch")).not.toBeDisabled());
  });

  it("quits the application", async () => {
    const onQuit = vi.fn();
    render(<ApplicationSection onQuit={onQuit} />);

    fireEvent.click(screen.getByRole("button", { name: "退出" }));

    await waitFor(() => expect(quitApp).toHaveBeenCalledTimes(1));
    expect(onQuit).toHaveBeenCalledTimes(1);
  });

  /// Outside Tauri the command is unavailable; clicking must not throw.
  it("survives a quit failure outside Tauri", async () => {
    quitApp.mockRejectedValue(new Error("not tauri"));
    render(<ApplicationSection />);

    fireEvent.click(screen.getByRole("button", { name: "退出" }));
    await waitFor(() => expect(quitApp).toHaveBeenCalledTimes(1));
  });

  it("loads the actual autostart state", async () => {
    getAutostart.mockResolvedValue(true);
    render(<ApplicationSection />);
    await waitFor(() => expect(screen.getByRole("switch")).toBeChecked());
    expect(setAutostart).not.toHaveBeenCalled();
  });

  it("enables and disables autostart through the backend", async () => {
    render(<ApplicationSection />);
    const toggle = screen.getByRole("switch", { name: "登录时自动启动" });
    await waitFor(() => expect(toggle).not.toBeDisabled());
    fireEvent.click(toggle);
    await waitFor(() => expect(toggle).toBeChecked());
    expect(setAutostart).toHaveBeenLastCalledWith(true);
    await waitFor(() => expect(toggle).not.toBeDisabled());
    fireEvent.click(toggle);
    await waitFor(() => expect(toggle).not.toBeChecked());
    expect(setAutostart).toHaveBeenLastCalledWith(false);
  });

  it("keeps the previous value when saving fails", async () => {
    getAutostart.mockResolvedValue(true);
    setAutostart.mockRejectedValue(new Error("read-only filesystem"));
    render(<ApplicationSection />);
    const toggle = screen.getByRole("switch");
    await waitFor(() => expect(toggle).not.toBeDisabled());
    fireEvent.click(toggle);
    expect(await screen.findByRole("alert")).toHaveTextContent("无法保存自启动设置");
    expect(toggle).toBeChecked();
  });

  it("disables the switch during loading and saving", async () => {
    let finishLoad!: (enabled: boolean) => void;
    let finishSave!: (enabled: boolean) => void;
    getAutostart.mockReturnValue(new Promise<boolean>((resolve) => { finishLoad = resolve; }));
    setAutostart.mockReturnValue(new Promise<boolean>((resolve) => { finishSave = resolve; }));
    render(<ApplicationSection />);
    const toggle = screen.getByRole("switch");
    expect(toggle).toBeDisabled();
    finishLoad(false);
    await waitFor(() => expect(toggle).not.toBeDisabled());
    fireEvent.click(toggle);
    expect(toggle).toBeDisabled();
    expect(toggle).not.toBeChecked();
    finishSave(true);
    await waitFor(() => expect(toggle).toBeChecked());
    await waitFor(() => expect(toggle).not.toBeDisabled());
  });

  it("reports a missing backend and allows retrying", async () => {
    getAutostart.mockRejectedValueOnce(new Error("backend unavailable"));
    render(<ApplicationSection />);
    expect(await screen.findByRole("alert")).toHaveTextContent("无法读取自启动设置");
    expect(screen.getByRole("switch")).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "重新读取自启动设置" }));
    await waitFor(() => expect(screen.getByRole("switch")).not.toBeDisabled());
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
