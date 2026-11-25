import { forwardRef, isValidElement } from "react";
import Dialog, { type DialogProps } from "@mui/material/Dialog";
import Slide from "@mui/material/Slide";
import type { TransitionProps } from "@mui/material/transitions";
import { motionDuration, UI_MOTION_EASING } from "../motion";
import type { Appearance } from "../theme";

export type DialogMotion = Pick<Appearance, "animationSpeed" | "animationsEnabled">;

const SlideUp = forwardRef<HTMLElement, TransitionProps>(function SlideUp({ children, ...props }, ref) {
  if (!isValidElement(children)) return null;
  return <Slide direction="up" ref={ref} {...props}>{children}</Slide>;
});

interface SlideDialogProps extends Omit<DialogProps, "slots" | "transitionDuration"> {
  motion: DialogMotion;
}

export function SlideDialog({ motion, slotProps, ...props }: SlideDialogProps) {
  return (
    <Dialog
      {...props}
      slots={{ transition: SlideUp }}
      transitionDuration={motionDuration(motion.animationSpeed, motion.animationsEnabled)}
      slotProps={{ ...slotProps, transition: { easing: UI_MOTION_EASING } }}
    />
  );
}
