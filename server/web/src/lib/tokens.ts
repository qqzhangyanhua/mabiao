export interface TokenSplit {
  input: number;
  output: number;
  cache_read: number;
  cache_creation: number;
  reasoning: number;
}

export const TOKEN_PARTS: { key: keyof TokenSplit; label: string; colorVar: string }[] = [
  { key: "input", label: "输入", colorVar: "var(--token-input)" },
  { key: "output", label: "输出", colorVar: "var(--token-output)" },
  { key: "cache_read", label: "缓存读", colorVar: "var(--token-cache-read)" },
  { key: "cache_creation", label: "缓存写", colorVar: "var(--token-cache-write)" },
  { key: "reasoning", label: "推理", colorVar: "var(--token-reasoning)" },
];

export function emptySplit(): TokenSplit {
  return { input: 0, output: 0, cache_read: 0, cache_creation: 0, reasoning: 0 };
}

export function addSplit(left: TokenSplit, right: TokenSplit): TokenSplit {
  return {
    input: left.input + right.input,
    output: left.output + right.output,
    cache_read: left.cache_read + right.cache_read,
    cache_creation: left.cache_creation + right.cache_creation,
    reasoning: left.reasoning + right.reasoning,
  };
}

/** 五口径合计；和 `total` 字段可能不一致（有的来源 total 另算）。 */
export function splitSum(split: TokenSplit): number {
  return (
    split.input + split.output + split.cache_read + split.cache_creation + split.reasoning
  );
}

/** 缓存读 ÷（输入 + 缓存读）。分母为 0 时没有命中率。 */
export function cacheHitRate(split: TokenSplit): number | null {
  const denom = split.input + split.cache_read;
  if (denom <= 0) return null;
  return split.cache_read / denom;
}
