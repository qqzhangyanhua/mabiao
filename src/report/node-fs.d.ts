/** vitest 跑在 Node；应用 tsconfig 没有 @types/node，只给 CSS 子集测试开这一处。 */
declare module "node:fs" {
  export function readFileSync(path: URL, encoding: "utf8"): string;
}
