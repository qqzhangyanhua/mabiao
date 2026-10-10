export type SourcePresenceRoot = {
  path: string;
  exists: boolean;
};

export type SourcePresenceRow = {
  source: string;
  application: string;
  roots: SourcePresenceRoot[];
};

export type SourcePresenceDto = {
  has_usage_records: boolean;
  rows: SourcePresenceRow[];
};

export function sourcePresenceSummary(rows: SourcePresenceRow[]): {
  found: SourcePresenceRow[];
  missing: SourcePresenceRow[];
} {
  const found: SourcePresenceRow[] = [];
  const missing: SourcePresenceRow[] = [];
  for (const row of rows) {
    if (row.roots.some((root) => root.exists)) {
      found.push(row);
    } else {
      missing.push(row);
    }
  }
  return { found, missing };
}
