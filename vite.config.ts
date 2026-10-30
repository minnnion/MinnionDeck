import { defineConfig, type Plugin } from "vite";
// @ts-expect-error type error without @types/node package
import process from "node:process";
// @ts-expect-error type error without @types/node package
import fs from "node:fs";
// @ts-expect-error type error without @types/node package
import path from "node:path";
const host = process.env.TAURI_DEV_HOST;

// The page scripts are plain (non-module) scripts split into files for editing.
// <script data-concat="src/ui/core"></script> is replaced with one inline script made of that
// folder's .js files joined in name order, so they run exactly like the single script they came
// from: top-level functions are shared and hoisted across files. With data-iife the files are
// wrapped in (() => { ... })(), so they share one private scope instead of the global one.
function concatScripts(): Plugin {
  const join = (dir: string) =>
    fs
      .readdirSync(dir)
      .filter((f: string) => f.endsWith(".js"))
      .sort()
      .map((f: string) => fs.readFileSync(path.join(dir, f), "utf8"))
      .join("");
  return {
    name: "concat-page-scripts",
    transformIndexHtml: {
      order: "pre",
      handler: (html) =>
        html.replace(/<script data-concat="([^"]+)"( data-iife)?><\/script>/g, (_m, dir: string, iife?: string) => {
          const body = join(path.resolve(dir));
          return `<script>\n${iife ? `(() => {\n${body}})();` : body.replace(/\n$/, "")}\n</script>`;
        }),
    },
    // index.html is rebuilt from these files, so editing one reloads the page
    handleHotUpdate({ file, server }) {
      if (/[\\/]src[\\/]ui[\\/].+\.js$/.test(file)) {
        server.ws.send({ type: "full-reload" });
        return [];
      }
    },
  };
}

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [concatScripts()],

  // Ship the UI's CSS exactly as written so the app matches the design file pixel for pixel
  build: {
    cssMinify: false,
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
