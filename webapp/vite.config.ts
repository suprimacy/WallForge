import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The dev server talks to the running WallForge backend so the React UI can
// be tested against the real API. The production build is plain static files
// that Flask serves from webapp/dist.
const API_TARGET = process.env.WALLFORGE_API || "http://127.0.0.1:5005";

export default defineConfig({
  plugins: [react()],
  base: "./",
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "es2022",
  },
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      "/api": { target: API_TARGET, changeOrigin: true },
      "/resources": { target: API_TARGET, changeOrigin: true },
    },
  },
});
