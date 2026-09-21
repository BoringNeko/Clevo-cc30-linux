import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { ApplicationSection } from "./ApplicationSection";

const quitApp = vi.fn();

vi.mock("../api/daemon", () => ({
  quitApp: () => quitApp(),
}));

describe("ApplicationSection", () => {
  beforeEach(() => {
    quitApp.mockReset().mockResolvedValue(undefined);
  });

  it("explains that closing hides to the tray", () => {
    render(<ApplicationSection />);
    expect(screen.getByText("关闭窗口的行为")).toBeTruthy();
    expect(screen.getByText(/系统托盘/)).toBeTruthy();
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
});
