import type { ComponentType, Ref } from "react";
import { CinnabarSlipPoster } from "./cinnabarSlipPoster";
import { DuskBriefPoster } from "./duskBriefPoster";
import { FolioRuledPoster } from "./folioRuledPoster";
import type { WorkNotesPosterViewModel } from "./posterTypes";

export type WorkNotesPosterRenderProps = {
  data: WorkNotesPosterViewModel;
  posterRef?: Ref<HTMLElement | null>;
  posterId?: string;
};

export type WorkNotesPosterStyleSwatch = {
  background: string;
  accent: string;
};

export type WorkNotesPosterStyleComponent = ComponentType<WorkNotesPosterRenderProps>;

export const WORK_NOTES_POSTER_STYLES = [
  {
    id: "folio-ruled",
    label: "栏线手札",
    stylesheet: "folioRuledPoster.css",
    swatch: {
      background: "#f3ead4",
      accent: "#2c4a7c",
    },
    Component: FolioRuledPoster,
  },
  {
    id: "dusk-brief",
    label: "暮色简报",
    stylesheet: "duskBriefPoster.css",
    swatch: {
      background: "#161410",
      accent: "#d4b36a",
    },
    Component: DuskBriefPoster,
  },
  {
    id: "cinnabar-slip",
    label: "朱砂条",
    stylesheet: "cinnabarSlipPoster.css",
    swatch: {
      background: "#f7f1e8",
      accent: "#c23a2b",
    },
    Component: CinnabarSlipPoster,
  },
] as const;

export type WorkNotesPosterStyleId = (typeof WORK_NOTES_POSTER_STYLES)[number]["id"];

export const DEFAULT_WORK_NOTES_POSTER_STYLE_ID = "folio-ruled" satisfies WorkNotesPosterStyleId;

export type WorkNotesPosterStyle = {
  id: WorkNotesPosterStyleId;
  label: string;
  stylesheet: string;
  swatch: WorkNotesPosterStyleSwatch;
  Component: WorkNotesPosterStyleComponent;
};

const WORK_NOTES_POSTER_STYLE_BY_ID = new Map<string, WorkNotesPosterStyle>(
  WORK_NOTES_POSTER_STYLES.map((style) => [style.id, style]),
);

export function isWorkNotesPosterStyleId(value: unknown): value is WorkNotesPosterStyleId {
  return typeof value === "string" && WORK_NOTES_POSTER_STYLE_BY_ID.has(value);
}

export function resolveWorkNotesPosterStyleId(value: unknown): WorkNotesPosterStyleId {
  return isWorkNotesPosterStyleId(value) ? value : DEFAULT_WORK_NOTES_POSTER_STYLE_ID;
}

export function resolveWorkNotesPosterStyle(value: unknown): WorkNotesPosterStyle {
  return (
    WORK_NOTES_POSTER_STYLE_BY_ID.get(resolveWorkNotesPosterStyleId(value)) ??
    WORK_NOTES_POSTER_STYLES[0]
  );
}
