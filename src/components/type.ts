import type { ReactElement } from "react";
import type { ThemeMode } from "../hooks/useTheme";
import type { IconName } from "../icons";
import type { SettingsTabId } from "../lib/type";
import type {
  ConversationSessionRow,
  CursorSessionDetailDto,
  CursorSessionListRow,
  OfficialQuotaRow,
} from "../types";

export type ThemeOption = {
  value: ThemeMode;
  label: string;
  icon: IconName;
  note: string;
};

export type SettingsTabIcon = Record<SettingsTabId, IconName>;

export type ConversationExportFormat = "markdown" | "json";

export type ConversationJumpBarProps = {
  atTop: boolean;
  atBottom: boolean;
  unseenCount: number;
  onJumpTop: () => void;
  onJumpBottom: () => void;
};

export type SourceMark = {
  viewBox: string;
  body: ReactElement;
};

export type SourceIconProps = {
  source: string;
  size?: number;
};

export type SourceLabelProps = {
  source: string;
  fallback?: string;
  size?: number;
};

export type ConversationCatalogRowProps = {
  row: ConversationSessionRow;
  maxTotal: number;
  onOpen: (row: ConversationSessionRow) => void;
};

export type CursorSessionDetailProps = {
  detail: CursorSessionDetailDto;
  embedded?: boolean;
};

export type CursorSessionTableSelect = (row: CursorSessionListRow) => void;

export type ConversationOpenRequest = {
  id: string;
  source: string;
};

export type OfficialQuotaListProps = {
  rows: OfficialQuotaRow[];
  staleAfterMinutes?: number;
  compactReset?: boolean;
  arrangeable?: boolean;
  busyProvider?: string | null;
  onRefresh?: (provider: string) => void;
  onArrange?: () => void;
};

export type ConversationDetailHeadProps = {
  session: ConversationSessionRow;
  fileAvailable: boolean;
  breadcrumb: string | null;
  parentAvailable: boolean;
  exportFormat: ConversationExportFormat | null;
  exportStatus: string | null;
  exportError: boolean;
  exportDisabled: boolean;
  onBack: () => void;
  onExport: (format: ConversationExportFormat) => void;
};
