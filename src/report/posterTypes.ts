export type PosterSourceSlice = {
  label: string;
  pct: number;
  color: string;
};

export type PosterDayBar = {
  label: string;
  tokens: number;
};

export type PosterViewModel = {
  kicker: string;
  rangeLabel: string;
  totalTokensLabel: string;
  totalUnit: string;
  totalCostLabel: string;
  nightShareComment: string;
  peakHoursComment: string;
  busiestDayLabel: string;
  busiestDayValue: string;
  topSessionLabel: string;
  topSessionValue: string;
  modelsLabel: string;
  modelsValue: string;
  days: PosterDayBar[];
  sources: PosterSourceSlice[];
};
