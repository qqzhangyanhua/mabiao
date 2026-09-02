import type { ReactNode } from "react";

export type HighlightParts = {
  before: string;
  match: string;
  after: string;
};

export function splitHighlight(text: string, query: string): HighlightParts | null {
  const needle = query.trim();
  if (!text || !needle) {
    return null;
  }
  const at = text.toLocaleLowerCase().indexOf(needle.toLocaleLowerCase());
  if (at < 0) {
    return null;
  }
  return {
    before: text.slice(0, at),
    match: text.slice(at, at + needle.length),
    after: text.slice(at + needle.length),
  };
}

export function HighlightedSnippet({
  text,
  query,
  className,
}: {
  text: string;
  query: string;
  className?: string;
}): ReactNode {
  const parts = splitHighlight(text, query);
  if (!parts) {
    return className ? <span className={className}>{text}</span> : text;
  }
  const marked = (
    <>
      {parts.before}
      <mark>{parts.match}</mark>
      {parts.after}
    </>
  );
  return className ? <span className={className}>{marked}</span> : marked;
}
