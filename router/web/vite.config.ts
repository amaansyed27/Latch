import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwind from "@tailwindcss/vite";

export default defineConfig({
  root: "web",
  plugins: [react(), tailwind()],
  build: { outDir: "../public", emptyOutDir: true },
  server: {
    host: "127.0.0.1",
    port: 5173,
    proxy: {
      "/api": "http://127.0.0.1:8787",
      "/oauth": "http://127.0.0.1:8787",
    },
  },
});
