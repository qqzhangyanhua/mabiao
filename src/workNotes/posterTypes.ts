export type WorkNotesPosterMetricId = "sessions" | "projects" | "active_days" | "tokens";

export type WorkNotesPosterMetric = {
  id: WorkNotesPosterMetricId;
  label: string;
  value: string;
};

export type WorkNotesPosterEntry = {
  title: string;
  detail: string;
  project: string | null;
};

export type WorkNotesPosterViewModel = {
  kicker: string;
  rangeLabel: string;
  headline: string;
  closing: string;
  entries: WorkNotesPosterEntry[];
  metrics: WorkNotesPosterMetric[];
  skippedLabel: string | null;
};
