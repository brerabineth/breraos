import { defineConfig } from "vite";

export default defineConfig({
  root: ".",
  base: "./",
  build: {
    target: "es2022",
    outDir: "dist",
    assetsInlineLimit: 8192,
    sourcemap: false,
  },
  server: {
    port: 5173,
    strictPort: true,
  },
  clearScreen: false,
});
