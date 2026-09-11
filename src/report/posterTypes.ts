export type PosterSourceSlice = {
  label: string;
  pct: number;
  color: string;
};

export type PosterDayBar = {
  label: string;
  tokens: number;
};

export type PosterStat =
  | { kind: "busiest_day"; label: string; value: string }
  | { kind: "models"; label: string; value: string; items: string[] }
  | { kind: "top_session"; label: string; value: string; amount: string; project: string | null };

export function findPosterStat<K extends PosterStat["kind"]>(
  stats: readonly PosterStat[],
  kind: K,
): Extract<PosterStat, { kind: K }> | undefined {
  return stats.find((stat): stat is Extract<PosterStat, { kind: K }> => stat.kind === kind);
}

export type PosterCursorAccount = {
  tokensLabel: string;
  costLabel: string | null;
  modelsLabel: string | null;
  note: string;
  line: string;
};

export type PosterViewModel = {
  kicker: string;
  rangeLabel: string;
  totalTokensLabel: string;
  totalUnit: string;
  totalCostLabel: string | null;
  comments: string[];
  days: PosterDayBar[];
  sources: PosterSourceSlice[];
  stats: PosterStat[];
  cursorAccount: PosterCursorAccount | null;
};

/** 画布海报把 Cursor 分区收成评语行；没有账号用量时原样返回。 */
export function posterNarrativeLines(data: PosterViewModel): string[] {
  if (data.cursorAccount == null) {
    return data.comments;
  }
  return [...data.comments, data.cursorAccount.line];
}
