import { expect, it } from "vitest";
import { render } from "@testing-library/react";
import { KeyboardColorPreview } from "./KeyboardColorPreview";

it("shows the selected static colour without an animation clock", () => {
  const view = render(
    <KeyboardColorPreview mode="static" color={[255, 0, 0]} brightness={100} />,
  );
  const root = view.getByLabelText("键盘颜色预览");

  expect(root).toBeTruthy();
  expect(root.style.getPropertyValue("--preview-r")).toBe("255");
  expect(root.style.getPropertyValue("--preview-g")).toBe("0");
  expect(root.style.getPropertyValue("--preview-b")).toBe("0");
});
