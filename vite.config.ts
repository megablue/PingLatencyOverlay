import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const host = process.env.TAURI_DEV_HOST;

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [react()],

  // Tauri expects a fixed port and fails if it is not available.
  // NOTE: port 1420 sits inside a Windows-reserved TCP range on some machines
  // (see `netsh interface ipv4 show excludedportrange protocol=tcp`), which
  // makes Vite fail with EACCES. 14200 is outside the common reserved ranges.
  clearScreen: false,
  server: {
    port: 14200,
    strictPort: true,
    host: host || "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 14201,
        }
      : undefined,
    watch: {
      // Never watch the Rust side; Tauri handles that.
      ignored: ["**/src-tauri/**"],
    },
  },
});
