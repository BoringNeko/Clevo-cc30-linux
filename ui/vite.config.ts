import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri expects a fixed dev port and its own host.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // Do not watch the Rust side; tauri dev rebuilds it separately.
      ignored: ["**/src-tauri/**"],
    },
  },
});
