import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import path from "path";
import { readFileSync } from "fs";

const tauriConf = JSON.parse(
  readFileSync(
    new URL("./src-tauri/tauri.conf.json", import.meta.url),
    "utf-8",
  ),
);
const appVersion: string = tauriConf.version ?? "0.1.0";

export default defineConfig({
  plugins: [vue()],
  define: {
    __APP_VERSION__: JSON.stringify(appVersion),
  },
  root: ".",
  build: {
    outDir: "dist-client",
    emptyOutDir: true,
    rollupOptions: {
      input: path.resolve(__dirname, "client.html"),
    },
  },
});
