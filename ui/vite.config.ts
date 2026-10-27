/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import brand from "../brand/brand.json" with { type: "json" };

// Tauri loads the dev server on a fixed port and the built files from dist/.
// The dev server may also read ../brand, where brand.json and the logo art live.
export default defineConfig({
  plugins: [
    react(),
    {
      name: "brand-title",
      transformIndexHtml: (html) => html.replaceAll("%PRODUCT_NAME%", brand.productName),
    },
  ],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    fs: { allow: [".", "../brand"] },
  },
  build: { outDir: "dist", target: "es2022" },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/setupTests.ts"],
  },
});
