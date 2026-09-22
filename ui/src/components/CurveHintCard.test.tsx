import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { CurveHintCard } from "./CurveHintCard";

const palette = {
  primary: [120, 200, 255] as [number, number, number],
  secondary: [180, 120, 255] as [number, number, number],
  swatches: [] as Array<[number, number, number]>,
};

describe("CurveHintCard", () => {
  it("tells the user to switch to the customize fan mode", () => {
    render(<CurveHintCard palette={palette} />);
    expect(screen.getAllByText("customize").length).toBeGreaterThan(0);
    expect(
      screen.getByText(/其他模式下固件不使用自定义曲线/),
    ).toBeTruthy();
  });

  it("keeps the curve card's title so the section stays recognisable", () => {
    render(<CurveHintCard palette={palette} />);
    expect(screen.getByText("风扇曲线")).toBeTruthy();
  });
});
