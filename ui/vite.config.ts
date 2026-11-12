/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import brand from "../brand/brand.json" with { type: "json" };

// Tauri loads the dev server on a fixed port and the built files from dist/.
// The dev server may also read ../brand, where brand.json and the logo art live.
// `nyx-desk` is the desktop companion's own see-through window page.
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
  // licenses.md ships the notices of every bundled package (MIT and the like
  // ask for theirs to travel with the code).
  build: {
    outDir: "dist",
    target: "es2022",
    license: { fileName: "licenses.md" },
    rollupOptions: {
      input: {
        main: "index.html",
        "nyx-desk": "nyx-desk.html",
      },
    },
  },
  test: {
    environment: "jsdom",
    globals: true,
    // Vitest swaps CSS imports for empty strings; contrast.test.ts reads the real theme sheet.
    css: { include: [/styles\.css/] },
    setupFiles: ["./src/setupTests.ts"],
    // Workers stay up between files, so each loads vitest, jsdom and the
    // test libraries once; every file still gets a fresh jsdom window in its
    // own VM context. Per-file setup was ~42% of the run (40 s → 17 s locally).
    pool: "vmThreads",
  },
});
