import "@testing-library/jest-dom/vitest";

// jsdom no longer ships a working localStorage in this version; provide a
// minimal in-memory implementation so the settings hooks can be tested.
if (typeof window !== "undefined" && !window.localStorage) {
  const store = new Map<string, string>();
  const localStorageMock: Storage = {
    get length() {
      return store.size;
    },
    clear: () => store.clear(),
    getItem: (key) => (store.has(key) ? store.get(key)! : null),
    key: (index) => Array.from(store.keys())[index] ?? null,
    removeItem: (key) => void store.delete(key),
    setItem: (key, value) => void store.set(key, String(value)),
  };
  Object.defineProperty(window, "localStorage", { value: localStorageMock, configurable: true });
}

// jsdom 25 implements no PointerEvent at all, so `fireEvent.pointerDown` never
// reaches a React `onPointerDown` and the curve editor's drag cannot be tested.
// The curve card only reads clientX/clientY and pointerId off the event, so a
// class extending MouseEvent (which jsdom does implement) is enough. Also add
// the pointer-capture methods the card calls on grab, which jsdom lacks.
if (typeof window !== "undefined" && !("PointerEvent" in window)) {
  class PointerEventPolyfill extends MouseEvent {
    public readonly pointerId: number;
    public readonly pointerType: string;
    constructor(type: string, params: PointerEventInit = {}) {
      super(type, params);
      this.pointerId = params.pointerId ?? 0;
      this.pointerType = params.pointerType ?? "mouse";
    }
  }
  Object.defineProperty(window, "PointerEvent", {
    value: PointerEventPolyfill,
    configurable: true,
  });
}

if (typeof Element !== "undefined") {
  const proto = Element.prototype as Element & {
    setPointerCapture?: (id: number) => void;
    releasePointerCapture?: (id: number) => void;
    hasPointerCapture?: (id: number) => boolean;
  };
  proto.setPointerCapture ??= () => {};
  proto.releasePointerCapture ??= () => {};
  proto.hasPointerCapture ??= () => false;
}
