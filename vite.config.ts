import { defineConfig } from "vite";

// Tauri expects a fixed dev port and must not have Vite clear its output.
export default defineConfig({
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  // One local bundle, loaded from disk: size warnings are noise here.
  build: { target: "safari16", chunkSizeWarningLimit: 1500 },
});
