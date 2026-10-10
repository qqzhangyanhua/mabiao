import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// 开发时把 API 代理到本机默认端口的 mabiao-server；生产由服务端用 --web-dir 直接托管 dist。
export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    proxy: {
      "/api": "http://127.0.0.1:8080",
    },
  },
});
