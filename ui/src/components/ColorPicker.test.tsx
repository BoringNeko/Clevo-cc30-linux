import { describe, expect, it } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ColorPicker } from "./ColorPicker";

function setup(value = "#78c8ff", extra: Record<string, unknown> = {}) {
  const changes: string[] = [];
  render(
    <ColorPicker
      value={value}
      onChange={(hex) => changes.push(hex)}
      swatches={[
        [120, 200, 255],
        [180, 120, 255],
      ]}
      label="强调色"
      {...extra}
    />,
  );
  return { changes };
}

describe("ColorPicker", () => {
  it("shows a trigger labelled with its accessible name", () => {
    setup();
    expect(screen.getByRole("button", { name: "强调色" })).toBeTruthy();
  });

  it("opens a dialog popover on click", async () => {
    const user = userEvent.setup();
    setup();
    await user.click(screen.getByRole("button", { name: "强调色" }));
    expect(screen.getByLabelText("十六进制颜色")).toBeTruthy();
    expect(screen.getByText("取色板")).toBeTruthy();
  });

  it("reports the wallpaper swatch colour when clicked", async () => {
    const user = userEvent.setup();
    const { changes } = setup();
    await user.click(screen.getByRole("button", { name: "强调色" }));
    await user.click(screen.getByRole("button", { name: "使用颜色 #b478ff" }));
    expect(changes).toContain("#b478ff");
  });

  it("reports a valid hex typed into the field", async () => {
    const user = userEvent.setup();
    const { changes } = setup();
    await user.click(screen.getByRole("button", { name: "强调色" }));
    const field = screen.getByLabelText("十六进制颜色");
    fireEvent.change(field, { target: { value: "#aa0011" } });
    expect(changes).toContain("#aa0011");
  });

  it("ignores a malformed hex entry", async () => {
    const user = userEvent.setup();
    const { changes } = setup();
    await user.click(screen.getByRole("button", { name: "强调色" }));
    const field = screen.getByLabelText("十六进制颜色");
    fireEvent.change(field, { target: { value: "#zzz" } });
    expect(changes).toEqual([]);
  });

  it("picks a colour by dragging on the saturation/value square", async () => {
    const user = userEvent.setup();
    const { changes } = setup();
    await user.click(screen.getByRole("button", { name: "强调色" }));
    const square = document.querySelector<HTMLElement>("[data-testid=sv-square]");
    expect(square).toBeTruthy();
    // jsdom has no layout, so give the square a size before dragging across it.
    square!.getBoundingClientRect = () =>
      ({ left: 0, top: 0, width: 200, height: 130, right: 200, bottom: 130, x: 0, y: 0, toJSON: () => ({}) }) as DOMRect;
    fireEvent.pointerDown(square!, { clientX: 200, clientY: 0 });
    expect(changes.length).toBeGreaterThan(0);
  });

  it("offers a reset action when onReset is provided", async () => {
    const user = userEvent.setup();
    let reset = false;
    render(
      <ColorPicker
        value="#123456"
        onChange={() => {}}
        label="强调色"
        onReset={() => {
          reset = true;
        }}
      />,
    );
    await user.click(screen.getByRole("button", { name: "强调色" }));
    await user.click(screen.getByRole("button", { name: "默认" }));
    expect(reset).toBe(true);
  });
});
