import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import path from "path";

export default defineConfig({
  plugins: [vue()],
  root: ".",
  build: {
    outDir: "dist-client",
    emptyOutDir: true,
    rollupOptions: {
      input: path.resolve(__dirname, "client.html"),
    },
  },
});
