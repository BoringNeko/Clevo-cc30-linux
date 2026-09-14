import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// Kept separate from vite.config.ts so the app build config does not depend on
// vitest's bundled Vite types.
export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    // A real origin is required for jsdom to expose window.localStorage.
    environmentOptions: { jsdom: { url: "http://localhost/" } },
    globals: true,
    setupFiles: ["./src/test-setup.ts"],
  },
});
