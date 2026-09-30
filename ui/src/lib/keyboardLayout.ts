// Compact 75% keyboard layout used by the RGB studio preview.
//
// Every row sums to exactly 15 units so the board lines up cleanly; the layout
// is purely a visual stand-in for the real 6x20 matrix the daemon reports,
// which does not describe key shapes. `code` matches `KeyboardEvent.code` so a
// physical keypress can light the matching cap.

/** One keycap: legend, physical code and width in units (1u = one standard key). */
export interface KeyDef {
  label: string;
  code: string;
  w: number;
}

/** 1u in CSS pixels. Sized so the 75% board fits the fixed design surface. */
export const KEY_UNIT_PX = 44;
/** Gap between keys in CSS pixels. */
export const KEY_GAP_PX = 5;

export const KEYBOARD_LAYOUT: KeyDef[][] = [
  [
    { label: "ESC", code: "Escape", w: 1 },
    { label: "F1", code: "F1", w: 1 },
    { label: "F2", code: "F2", w: 1 },
    { label: "F3", code: "F3", w: 1 },
    { label: "F4", code: "F4", w: 1 },
    { label: "F5", code: "F5", w: 1 },
    { label: "F6", code: "F6", w: 1 },
    { label: "F7", code: "F7", w: 1 },
    { label: "F8", code: "F8", w: 1 },
    { label: "F9", code: "F9", w: 1 },
    { label: "F10", code: "F10", w: 1 },
    { label: "F11", code: "F11", w: 1 },
    { label: "F12", code: "F12", w: 1 },
    { label: "PRT", code: "PrintScreen", w: 1 },
    { label: "DEL", code: "Delete", w: 1 },
  ],
  [
    { label: "~", code: "Backquote", w: 1 },
    { label: "1", code: "Digit1", w: 1 },
    { label: "2", code: "Digit2", w: 1 },
    { label: "3", code: "Digit3", w: 1 },
    { label: "4", code: "Digit4", w: 1 },
    { label: "5", code: "Digit5", w: 1 },
    { label: "6", code: "Digit6", w: 1 },
    { label: "7", code: "Digit7", w: 1 },
    { label: "8", code: "Digit8", w: 1 },
    { label: "9", code: "Digit9", w: 1 },
    { label: "0", code: "Digit0", w: 1 },
    { label: "-", code: "Minus", w: 1 },
    { label: "=", code: "Equal", w: 1 },
    { label: "BKSP", code: "Backspace", w: 2 },
  ],
  [
    { label: "TAB", code: "Tab", w: 1.5 },
    { label: "Q", code: "KeyQ", w: 1 },
    { label: "W", code: "KeyW", w: 1 },
    { label: "E", code: "KeyE", w: 1 },
    { label: "R", code: "KeyR", w: 1 },
    { label: "T", code: "KeyT", w: 1 },
    { label: "Y", code: "KeyY", w: 1 },
    { label: "U", code: "KeyU", w: 1 },
    { label: "I", code: "KeyI", w: 1 },
    { label: "O", code: "KeyO", w: 1 },
    { label: "P", code: "KeyP", w: 1 },
    { label: "[", code: "BracketLeft", w: 1 },
    { label: "]", code: "BracketRight", w: 1 },
    { label: "\\", code: "Backslash", w: 1.5 },
  ],
  [
    { label: "CAPS", code: "CapsLock", w: 1.75 },
    { label: "A", code: "KeyA", w: 1 },
    { label: "S", code: "KeyS", w: 1 },
    { label: "D", code: "KeyD", w: 1 },
    { label: "F", code: "KeyF", w: 1 },
    { label: "G", code: "KeyG", w: 1 },
    { label: "H", code: "KeyH", w: 1 },
    { label: "J", code: "KeyJ", w: 1 },
    { label: "K", code: "KeyK", w: 1 },
    { label: "L", code: "KeyL", w: 1 },
    { label: ";", code: "Semicolon", w: 1 },
    { label: "'", code: "Quote", w: 1 },
    { label: "ENTER", code: "Enter", w: 2.25 },
  ],
  [
    { label: "SHIFT", code: "ShiftLeft", w: 2.25 },
    { label: "Z", code: "KeyZ", w: 1 },
    { label: "X", code: "KeyX", w: 1 },
    { label: "C", code: "KeyC", w: 1 },
    { label: "V", code: "KeyV", w: 1 },
    { label: "B", code: "KeyB", w: 1 },
    { label: "N", code: "KeyN", w: 1 },
    { label: "M", code: "KeyM", w: 1 },
    { label: ",", code: "Comma", w: 1 },
    { label: ".", code: "Period", w: 1 },
    { label: "/", code: "Slash", w: 1 },
    { label: "SHIFT", code: "ShiftRight", w: 1.75 },
    { label: "▲", code: "ArrowUp", w: 1 },
  ],
  [
    { label: "CTRL", code: "ControlLeft", w: 1.25 },
    { label: "WIN", code: "MetaLeft", w: 1.25 },
    { label: "ALT", code: "AltLeft", w: 1.25 },
    { label: "SPACE", code: "Space", w: 6.25 },
    { label: "ALT", code: "AltRight", w: 1 },
    { label: "FN", code: "ContextMenu", w: 1 },
    { label: "◄", code: "ArrowLeft", w: 1 },
    { label: "▼", code: "ArrowDown", w: 1 },
    { label: "►", code: "ArrowRight", w: 1 },
  ],
];
