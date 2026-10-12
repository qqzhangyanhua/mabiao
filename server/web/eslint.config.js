import js from "@eslint/js";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";
import tseslint from "typescript-eslint";
import eslintConfigPrettier from "eslint-config-prettier";

export default tseslint.config(
  { ignores: ["dist", "node_modules"] },
  {
    extends: [js.configs.recommended, ...tseslint.configs.recommendedTypeChecked],
    files: ["**/*.{ts,tsx}"],
    languageOptions: {
      ecmaVersion: 2022,
      globals: globals.browser,
      parserOptions: {
        project: ["./tsconfig.json", "./tsconfig.node.json"],
        tsconfigRootDir: import.meta.dirname,
      },
    },
    plugins: { "react-hooks": reactHooks },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "@typescript-eslint/no-explicit-any": "error",
      "@typescript-eslint/no-unused-vars": ["error", { argsIgnorePattern: "^_" }],
      "@typescript-eslint/no-misused-promises": [
        "error",
        { checksVoidReturn: { attributes: false } },
      ],
      // 单组件 ≤ 400 行。
      "max-lines": ["error", { max: 400, skipBlankLines: true, skipComments: true }],
      // 独立前端包，禁止引用桌面端 src/（Docker 与 vitest 都解析不到）。
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            {
              group: ["../../../../src/**", "../../../../../src/**"],
              message: "server/web 不得引用桌面端 src/，组件请放在本包内。",
            },
          ],
        },
      ],
    },
  },
  eslintConfigPrettier,
);
