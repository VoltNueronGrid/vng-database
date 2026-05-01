import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "path";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
    // Proxy VNG API calls in browser dev mode to avoid CORS.
    // In production Tauri, fetch goes directly to the configured server.
    proxy: {
      "/api": {
        target: process.env.VITE_VNG_DEV_URL || "http://127.0.0.1:8080",
        changeOrigin: true,
      },
      "/health": {
        target: process.env.VITE_VNG_DEV_URL || "http://127.0.0.1:8080",
        changeOrigin: true,
      },
    },
    allowedHosts: [
      "ff0d-2405-201-c033-20fa-fcd3-b1ad-2801-5d06.ngrok-free.app",
      ".ngrok-free.app"
    ],
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    target:
      process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome105" : "safari13",
    minify: !process.env.TAURI_ENV_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
});
