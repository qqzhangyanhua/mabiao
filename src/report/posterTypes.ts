export type PosterSourceSlice = {
  label: string;
  pct: number;
  color: string;
};

export type PosterDayBar = {
  label: string;
  tokens: number;
};

export type PosterStat = {
  label: string;
  value: string;
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
};
