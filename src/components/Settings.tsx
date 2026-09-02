import { useEffect, useState, type KeyboardEvent } from "react";
import type { ThemeMode } from "../hooks/useTheme";
import type { OverviewLayout } from "../lib/overviewLayout";
import { hashForTab, SETTINGS_TABS, tabFromHash } from "../lib/settingsTabs";
import type { SettingsTabId } from "../lib/type";
import { Icon } from "../icons";
import type {
  BudgetStatusDto,
  IngestReport,
  OfficialQuotaDto,
  PriceTable,
  SourceDiagnostic,
} from "../types";
import { AppearanceSettingsPanel } from "./AppearanceSettingsPanel";
import { BackupPanel } from "./BackupPanel";
import { ConversationIndexPanel } from "./ConversationIndexPanel";
import { BudgetPanel } from "./BudgetPanel";
import { CursorAccountSettingsPanel } from "./CursorAccountSettingsPanel";
import { CustomQuotaProviderPanel } from "./CustomQuotaProviderPanel";
import { LiteLlmSnapshotPanel } from "./LiteLlmSnapshotPanel";
import { OfficialQuotaSettingsPanel } from "./OfficialQuotaSettingsPanel";
import { OverviewLayoutPanel } from "./OverviewLayoutPanel";
import { PriceConfigPanel } from "./PriceConfigPanel";
import { PricePresetPanel } from "./PricePresetPanel";
import { ScanPathPanel } from "./ScanPathPanel";
import { SourceDiagnosticsPanel } from "./SourceDiagnosticsPanel";
import { UnpricedDiagnosisPanel } from "./UnpricedDiagnosisPanel";
import type { SettingsTabIcon } from "./type";

const TAB_ICONS: SettingsTabIcon = {
  general: "monitor",
  sources: "source",
  quota: "cost",
  pricing: "tokens",
  cursor: "cursor",
};

export function Settings({
  prices,
  diagnostics,
  ingestReport,
  rebuilding,
  purging,
  operationBusy,
  observedModels,
  budgetStatus,
  savingBudget,
  themeMode,
  autoRefresh,
  cursorAccountAutoRefresh,
  onChange,
  onSave,
  onRebuild,
  onPurgeArchived,
  onSnapshotRefreshed,
  onSaveBudget,
  officialQuota,
  onOfficialQuota,
  onQuotaError,
  overviewLayout,
  onOverviewLayoutChange,
  onThemeModeChange,
  onAutoRefreshChange,
  onCursorAccountAutoRefreshChange,
}: {
  prices: PriceTable;
  diagnostics: SourceDiagnostic[];
  ingestReport: IngestReport | null;
  rebuilding: string | null;
  purging: string | null;
  operationBusy: boolean;
  observedModels: string[];
  budgetStatus: BudgetStatusDto | null;
  savingBudget: boolean;
  themeMode: ThemeMode;
  autoRefresh: string;
  cursorAccountAutoRefresh: boolean;
  onChange: (prices: PriceTable) => void;
  onSave: () => void | Promise<void>;
  onRebuild: (source: string | null) => void;
  onPurgeArchived: (source: string | null) => void;
  onSnapshotRefreshed: () => void;
  onSaveBudget: (monthlyUsd: number | null) => void;
  officialQuota: OfficialQuotaDto | null;
  onOfficialQuota: (value: OfficialQuotaDto) => void;
  onQuotaError: (error: unknown) => void;
  overviewLayout: OverviewLayout;
  onOverviewLayoutChange: (layout: OverviewLayout) => void;
  onThemeModeChange: (mode: ThemeMode) => void;
  onAutoRefreshChange: (value: string) => void;
  onCursorAccountAutoRefreshChange: (value: boolean) => void;
}) {
  const detectedSources = diagnostics.filter((row) => row.detected).map((row) => row.source);
  const [tab, setTab] = useState<SettingsTabId>(() => tabFromHash(window.location.hash));
  const [anchor, setAnchor] = useState<string | null>(() => {
    const hash = window.location.hash.replace(/^#/, "");
    return hash.startsWith("settings-") ? hash : null;
  });
  const [diagnosisEpoch, setDiagnosisEpoch] = useState(0);
  const [prefillKey, setPrefillKey] = useState<string | null>(null);

  useEffect(() => {
    function applyHash() {
      const hash = window.location.hash.replace(/^#/, "");
      setTab(tabFromHash(hash));
      setAnchor(hash.startsWith("settings-") ? hash : null);
    }
    window.addEventListener("hashchange", applyHash);
    applyHash();
    return () => window.removeEventListener("hashchange", applyHash);
  }, []);

  useEffect(() => {
    if (!anchor) {
      return;
    }
    document.getElementById(anchor)?.scrollIntoView({ block: "start" });
  }, [tab, anchor]);

  function selectTab(id: SettingsTabId) {
    setTab(id);
    setAnchor(null);
    const next = hashForTab(id);
    if (window.location.hash.replace(/^#/, "") !== next) {
      window.history.replaceState(null, "", `#${next}`);
    }
    document.querySelector("main.main")?.scrollTo({ top: 0 });
  }

  function onTabListKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key !== "ArrowRight" && event.key !== "ArrowLeft") {
      return;
    }
    const index = SETTINGS_TABS.findIndex((item) => item.id === tab);
    if (index < 0) {
      return;
    }
    event.preventDefault();
    const delta = event.key === "ArrowRight" ? 1 : -1;
    const next = SETTINGS_TABS[(index + delta + SETTINGS_TABS.length) % SETTINGS_TABS.length];
    selectTab(next.id);
    event.currentTarget
      .querySelector<HTMLButtonElement>(`[data-settings-tab="${next.id}"]`)
      ?.focus();
  }

  return (
    <div className="stack">
      <div
        className="settings-tabs"
        role="tablist"
        aria-label="设置分类"
        onKeyDown={onTabListKeyDown}
      >
        {SETTINGS_TABS.map((item) => {
          const active = item.id === tab;
          return (
            <button
              key={item.id}
              type="button"
              role="tab"
              data-settings-tab={item.id}
              id={`settings-tab-${item.id}`}
              aria-selected={active}
              aria-controls={`settings-panel-${item.id}`}
              tabIndex={active ? 0 : -1}
              className={active ? "settings-tab active" : "settings-tab"}
              onClick={() => selectTab(item.id)}
            >
              <Icon name={TAB_ICONS[item.id]} size={14} />
              <span>{item.label}</span>
            </button>
          );
        })}
      </div>
      <div
        role="tabpanel"
        id={`settings-panel-${tab}`}
        aria-labelledby={`settings-tab-${tab}`}
        className="stack"
      >
        {tab === "general" ? (
          <>
            <AppearanceSettingsPanel
              themeMode={themeMode}
              autoRefresh={autoRefresh}
              onThemeModeChange={onThemeModeChange}
              onAutoRefreshChange={onAutoRefreshChange}
            />
            <OverviewLayoutPanel
              layout={overviewLayout}
              detectedSources={detectedSources}
              presentSources={detectedSources}
              onChange={onOverviewLayoutChange}
            />
          </>
        ) : null}
        {tab === "sources" ? (
          <>
            <ScanPathPanel operationBusy={operationBusy} onSaved={onSnapshotRefreshed} />
            <SourceDiagnosticsPanel
              diagnostics={diagnostics}
              ingestReport={ingestReport}
              rebuilding={rebuilding}
              purging={purging}
              operationBusy={operationBusy}
              onRebuild={onRebuild}
              onPurgeArchived={onPurgeArchived}
            />
            <ConversationIndexPanel />
            <BackupPanel onRestored={onSnapshotRefreshed} />
          </>
        ) : null}
        {tab === "quota" ? (
          <>
            <OfficialQuotaSettingsPanel
              quota={officialQuota}
              onQuota={onOfficialQuota}
              onError={onQuotaError}
            />
            <CustomQuotaProviderPanel onQuota={onOfficialQuota} />
          </>
        ) : null}
        {tab === "pricing" ? (
          <>
            <BudgetPanel status={budgetStatus} saving={savingBudget} onSave={onSaveBudget} />
            <LiteLlmSnapshotPanel
              onRefreshed={() => {
                onSnapshotRefreshed();
                setDiagnosisEpoch((value) => value + 1);
              }}
            />
            <UnpricedDiagnosisPanel
              key={diagnosisEpoch}
              prices={prices}
              onChange={onChange}
              onPrefillHighlight={setPrefillKey}
            />
            <PricePresetPanel prices={prices} observedModels={observedModels} onChange={onChange} />
            <PriceConfigPanel
              prices={prices}
              highlightKey={prefillKey}
              onChange={onChange}
              onSave={() => {
                void Promise.resolve(onSave()).finally(() => {
                  setDiagnosisEpoch((value) => value + 1);
                  setPrefillKey(null);
                });
              }}
            />
          </>
        ) : null}
        {tab === "cursor" ? (
          <CursorAccountSettingsPanel
            autoRefresh={cursorAccountAutoRefresh}
            onAutoRefreshChange={onCursorAccountAutoRefreshChange}
          />
        ) : null}
      </div>
    </div>
  );
}
