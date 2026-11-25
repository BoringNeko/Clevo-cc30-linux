import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { getMuxStatus, setMuxMode } from "../api/daemon";
import { useMux } from "../hooks/useMux";
import type { ExtractedPalette } from "../lib/color";
import { DEFAULT_APPEARANCE } from "../theme";
import { MuxCard } from "./MuxCard";

vi.mock("../api/daemon", () => ({ getMuxStatus: vi.fn(), setMuxMode: vi.fn() }));

const palette: ExtractedPalette = {
  primary: [120, 200, 255],
  secondary: [110, 190, 255],
  swatches: [[180, 120, 255]],
};
const discrete = { current_mode: 2, supported_mask: 0x06, requested_mode: 0 };

function Dashboard() {
  return <MuxCard palette={palette} mux={useMux()} motion={DEFAULT_APPEARANCE} />;
}

beforeEach(() => vi.resetAllMocks());

it("selects a new target immediately and only prompts for the confirmed pending mode", async () => {
  vi.mocked(getMuxStatus)
    .mockResolvedValueOnce(discrete)
    .mockResolvedValueOnce({ ...discrete, requested_mode: 3 })
    .mockResolvedValueOnce({ ...discrete, requested_mode: 2 });
  let finishSubmission!: () => void;
  vi.mocked(setMuxMode)
    .mockImplementationOnce(() => new Promise<void>((resolve) => { finishSubmission = resolve; }))
    .mockResolvedValue(undefined);
  render(<Dashboard />);

  const hybrid = await screen.findByRole("button", { name: "混合模式" });
  const direct = screen.getByRole("button", { name: "独显直连", pressed: true });
  fireEvent.click(hybrid);
  expect(hybrid).toHaveAttribute("aria-pressed", "true");
  expect(hybrid).toHaveStyle({ backgroundColor: "rgba(110, 190, 255, 0.42)" });
  expect(direct).toHaveAttribute("aria-pressed", "false");
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(setMuxMode).toHaveBeenLastCalledWith("hybrid");
  finishSubmission();

  const dialog = await screen.findByRole("dialog", { name: "显卡模式待生效" });
  expect(dialog).toHaveAccessibleDescription("重启电脑后将切换为「混合模式」。");
  fireEvent.click(within(dialog).getByRole("button", { name: "知道了" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  expect(screen.getByText("待重启 · 切换为混合模式")).toBeInTheDocument();

  fireEvent.click(direct);
  expect(direct).toHaveAttribute("aria-pressed", "true");
  expect(direct).toHaveStyle({ backgroundColor: "rgba(120, 200, 255, 0.42)" });
  await waitFor(() => expect(screen.queryByRole("status")).not.toBeInTheDocument());
  expect(setMuxMode).toHaveBeenLastCalledWith("discrete");
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

it("blocks a mode excluded by the firmware mask", async () => {
  vi.mocked(getMuxStatus).mockResolvedValue({ ...discrete, supported_mask: 0x02 });
  render(<Dashboard />);

  const hybrid = await screen.findByRole("button", { name: "混合模式" });
  expect(hybrid).toBeDisabled();
  fireEvent.click(hybrid);
  expect(setMuxMode).not.toHaveBeenCalled();
});

it("reports a denied write without showing a reboot target", async () => {
  vi.mocked(getMuxStatus).mockResolvedValue(discrete);
  vi.mocked(setMuxMode).mockRejectedValue("AccessDenied");
  render(<Dashboard />);

  fireEvent.click(await screen.findByRole("button", { name: "混合模式" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("AccessDenied");
  expect(await screen.findByRole("button", { name: "独显直连", pressed: true })).toBeInTheDocument();
  expect(screen.queryByText(/待重启/)).not.toBeInTheDocument();
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});
