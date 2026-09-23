import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri expects a fixed dev port and its own host.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  // Emit relative asset paths. The bundle is loaded by two shells: Tauri's
  // custom protocol and Electron's `file://`. An absolute `/assets/...` URL
  // resolves to the filesystem root under `file://` and every script 404s, so
  // `./` is required for the Electron build (and is fine for Tauri).
  base: "./",
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // Do not watch the Rust side; tauri dev rebuilds it separately.
      ignored: ["**/src-tauri/**"],
    },
  },
});
