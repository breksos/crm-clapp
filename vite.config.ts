import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath, URL } from "node:url";

// The front-end half of clappkit is plain TypeScript behind an alias, not an npm
// package — React and @tauri-apps/api come from this app's own node_modules.
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@clappkit": fileURLToPath(new URL("./clappkit/web/index.ts", import.meta.url)),
    },
  },
  build: { outDir: "dist", emptyOutDir: true, target: "es2021" },
  // Tauri drives this; a browser tab is not the product.
  server: { port: 5173, strictPort: true },
  clearScreen: false,
});
