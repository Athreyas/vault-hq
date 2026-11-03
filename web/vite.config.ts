import { resolve } from "node:path";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Two pages, both bundled into the app: the main window and the quick-capture window.
// In development `npx tauri dev` loads them from this server (devUrl in tauri.conf.json).
export default defineConfig({
  root: import.meta.dirname,
  plugins: [react()],
  build: {
    outDir: "dist",
    emptyOutDir: true,
    rollupOptions: {
      input: {
        main: resolve(import.meta.dirname, "index.html"),
        capture: resolve(import.meta.dirname, "capture.html"),
      },
    },
  },
  server: { port: 5173, strictPort: true },
  clearScreen: false,
});
