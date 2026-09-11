import { defineConfig } from "vitest/config";

/**
 * 纯函数测试留在 node；只有 hook 测试启用 jsdom。
 * 不要把整个套件切到 DOM 环境。
 */
export default defineConfig({
  test: {
    projects: [
      {
        test: {
          name: "unit",
          environment: "node",
          include: ["src/**/*.test.ts"],
        },
      },
      {
        test: {
          name: "hooks",
          environment: "jsdom",
          include: ["src/**/*.hook.test.tsx"],
        },
      },
    ],
  },
});
