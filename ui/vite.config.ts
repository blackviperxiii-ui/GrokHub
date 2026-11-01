/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import brand from "../brand/brand.json" with { type: "json" };

// Tauri loads the dev server on a fixed port and the built files from dist/.
// The dev server may also read ../brand, where brand.json and the logo art live.
export default defineConfig({
  plugins: [
    react({
      // React Compiler (the native oxc port), only on components marked "use memo".
      // logDiagnostics names the reason when one is skipped instead of compiled.
      compiler: { compilationMode: "annotation", logDiagnostics: true },
      // The compiler pass would also rewrite `?raw` source text (brand.test.tsx
      // reads every file that way) and empties a `.d.ts` there.
      exclude: [/\/node_modules\//, /[?&]raw(?:&|$)/],
    }),
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
    // Vitest swaps CSS imports for empty strings; contrast.test.ts reads the real theme sheet.
    css: { include: [/styles\.css/] },
    setupFiles: ["./src/setupTests.ts"],
  },
});
