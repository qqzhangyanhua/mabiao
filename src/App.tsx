import { invoke } from "@tauri-apps/api/core";
import { Suspense, useEffect } from "react";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { LoadingOverlay } from "./components/LoadingOverlay";
import { Sidebar } from "./components/Sidebar";
import { Topbar } from "./components/Topbar";
import { useKeyboardShortcuts } from "./hooks/useKeyboardShortcuts";
import { useOverviewLayout } from "./hooks/useOverviewLayout";
import { useTheme } from "./hooks/useTheme";
import { useUsageData } from "./hooks/useUsageData";
import { clearDimensionFilters, withModelFilter, withProviderFilter } from "./lib/filterChips";
import { isOfficialProviderVisible, OFFICIAL_QUOTA_PROVIDER_IDS } from "./lib/overviewLayout";
import {
  LazyApplicationAnalytics,
  LazyBreakdown,
  LazyConversations,
  LazyCursorAccountUsagePanel,
  LazyCursorPanel,
  LazyCursorSessionPanel,
  LazyGlobalInstructionPanel,
  LazyOverview,
  LazySettings,
  LazyTrend,
  LazyWorkTimeline,
} from "./views/lazyViews";
import { ViewFallback } from "./views/ViewFallback";

export default function App() {
  const data = useUsageData();
  const { theme, mode: themeMode, setMode: setThemeMode } = useTheme();
  const { layout: overviewLayout, setLayout: setOverviewLayout } = useOverviewLayout();
  const { view } = data;
  const detectedSources = data.diagnostics.filter((row) => row.detected).map((row) => row.source);

  // 托盘额度面板没有自己的显示配置，复用这份「配置显示」——一处关掉两边同步隐藏。
  // 托盘运行在 Rust 进程里，够不到 localStorage，所以每次这份配置变化都顺手
  // 写一份到 official_quota.json，托盘下次打开/刷新面板时直接读那份文件。
  useEffect(() => {
    const quota = data.officialQuota;
    if (!quota) {
      return;
    }
    const hidden = OFFICIAL_QUOTA_PROVIDER_IDS.filter(
      (id) => !isOfficialProviderVisible(overviewLayout, id),
    );
    const current = quota.hidden_providers ?? [];
    const unchanged =
      hidden.length === current.length && hidden.every((id) => current.includes(id));
    if (unchanged) {
      return;
    }
    void invoke("save_official_quota_config", {
      config: { alerts_enabled: quota.alerts_enabled, hidden_providers: hidden },
    })
      .then(() => {
        data.setOfficialQuota((prev) => (prev ? { ...prev, hidden_providers: hidden } : prev));
      })
      .catch(() => {
        // 同步失败不影响主窗口本地显示，下次配置变化或重启应用时还会再推一次。
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 只需在官方额度可见性或已加载快照变化时同步，modules/quotaSources 与此无关
  }, [overviewLayout.officialProviders, data.officialQuota]);

  useKeyboardShortcuts({
    onNavigate: data.navigate,
    onRefresh: () => {
      void data.runIngest("刷新");
    },
    onClearFilters: () => data.applyFilter(clearDimensionFilters(data.filter)),
  });

  return (
    <div className="app">
      <Sidebar
        view={view}
        busy={data.busy}
        connected={data.connected}
        partial={Boolean(data.lastIngestReport?.partial_success)}
        status={data.status}
        onNavigate={data.navigate}
      />
      <div className="workspace">
        <Topbar
          key={view}
          view={view}
          filter={data.filter}
          preset={data.preset}
          options={data.options}
          disabled={data.loading}
          refreshDisabled={data.busy}
          onPreset={data.applyPreset}
          onChange={data.applyFilter}
          onRangeBack={data.canGoBack ? data.popRange : undefined}
          onRefresh={() => data.runIngest("刷新")}
        />
        <main className="main">
          <LoadingOverlay
            active={
              data.loading &&
              !data.viewHasData &&
              view !== "conversations" &&
              view !== "cursor" &&
              view !== "cursor-sessions" &&
              view !== "instructions" &&
              view !== "worktime"
            }
          >
            <ErrorBoundary fullscreen={false}>
              <Suspense fallback={<ViewFallback />}>
                {view === "overview" ? (
                  <LazyOverview
                    overview={data.overview}
                    billingWindows={data.billingWindows}
                    officialQuota={data.officialQuota}
                    cursorAccountUsage={data.cursorAccountUsage}
                    previous={data.previous}
                    trend={data.trend}
                    heatmap={data.heatmap}
                    heatmapRange={data.heatmapRange}
                    models={data.models}
                    projects={data.projects}
                    sessions={data.sessions}
                    grain={data.grain}
                    preset={data.preset}
                    updatedAt={data.updatedAt}
                    live={data.connected}
                    theme={theme}
                    onGrain={data.setGrain}
                    onOpenConversations={() => data.openConversations()}
                    onOpenUnpricedDiagnosis={data.openUnpricedDiagnosis}
                    onOpenCursor={() => data.navigate("cursor")}
                    onProjectClick={(project) =>
                      data.applyFilter({ ...data.filter, projects: [project] })
                    }
                    onRangeSelect={data.drillRange}
                    onRangeBack={data.canGoBack ? data.popRange : undefined}
                    onModelClick={(model) => data.applyFilter(withModelFilter(data.filter, model))}
                    onSessionClick={(session) => data.openConversations(session)}
                    layout={overviewLayout}
                    onLayoutChange={setOverviewLayout}
                    detectedSources={detectedSources}
                    onOfficialQuota={data.setOfficialQuota}
                    onQuotaError={data.reportError}
                  />
                ) : null}
                {view === "trend" ? (
                  <LazyTrend
                    grain={data.grain}
                    setGrain={data.setGrain}
                    points={data.trend}
                    theme={theme}
                    onRangeSelect={data.drillRange}
                    onRangeBack={data.canGoBack ? data.popRange : undefined}
                  />
                ) : null}
                {view === "application" ? (
                  <LazyApplicationAnalytics
                    analytics={data.applicationAnalytics}
                    grain={data.grain}
                    setGrain={data.setGrain}
                    theme={theme}
                  />
                ) : null}
                {["model", "provider", "project"].includes(view) ? (
                  <LazyBreakdown
                    title={
                      view === "model" ? "按模型" : view === "provider" ? "按接口" : "按项目"
                    }
                    icon={view === "model" ? "model" : view === "provider" ? "provider" : "project"}
                    rows={data.breakdown}
                    showProviderChannel={view === "provider"}
                    showVendorIcon={view === "model" || view === "provider"}
                    projectNames={view === "project"}
                    showCallDetails={view === "provider"}
                    filter={view === "provider" ? data.filter : undefined}
                    revision={String(data.sessionsRevision)}
                    theme={theme}
                    onProviderClick={
                      view === "provider"
                        ? (provider) => data.applyFilter(withProviderFilter(data.filter, provider))
                        : undefined
                    }
                    onOpenConversation={(session) => data.openConversations(session)}
                    onOpenUnpricedDiagnosis={data.openUnpricedDiagnosis}
                    onError={data.reportError}
                  />
                ) : null}
                {view === "cursor" ? (
                  <div className="stack">
                    <LazyCursorAccountUsagePanel theme={theme} />
                    <LazyCursorPanel
                      summary={data.codeVolume}
                      loading={data.codeVolumeLoading}
                      theme={theme}
                    />
                  </div>
                ) : null}
                {view === "cursor-sessions" ? (
                  <LazyCursorSessionPanel
                    summary={data.cursorSessionSummary}
                    loading={data.cursorSessionLoading}
                    theme={theme}
                    revision={data.sessionsRevision}
                    onError={data.reportError}
                    onOpenConversation={(session) => data.openConversations(session)}
                  />
                ) : null}
                {view === "conversations" ? (
                  <LazyConversations
                    filter={data.filter}
                    revision={data.sessionsRevision}
                    focus={data.conversationFocus}
                    onFocusConsumed={data.clearConversationFocus}
                    onError={data.reportError}
                  />
                ) : null}
                {view === "worktime" ? (
                  <LazyWorkTimeline
                    onSessionClick={(session) => data.openConversations(session)}
                  />
                ) : null}
                {view === "instructions" ? <LazyGlobalInstructionPanel /> : null}
                {view === "settings" ? (
                  <LazySettings
                    prices={data.prices}
                    diagnostics={data.diagnostics}
                    ingestReport={data.lastIngestReport}
                    rebuilding={data.rebuilding}
                    purging={data.purging}
                    operationBusy={data.busy}
                    observedModels={data.options.models}
                    budgetStatus={data.budgetStatus}
                    savingBudget={data.savingBudget}
                    onChange={data.setPrices}
                    onRebuild={data.runRebuild}
                    onPurgeArchived={data.runPurgeArchived}
                    onSave={async () => {
                      try {
                        await invoke("save_price_table", { prices: data.prices });
                        data.setStatus("单价已保存");
                      } catch (error) {
                        data.reportError(error);
                      }
                    }}
                    onSnapshotRefreshed={() => data.runIngest("刷新")}
                    onSaveBudget={(monthlyUsd: number | null) =>
                      data.saveBudget({ monthly_usd: monthlyUsd }).catch(() => undefined)
                    }
                    officialQuota={data.officialQuota}
                    onOfficialQuota={data.setOfficialQuota}
                    onQuotaError={data.reportError}
                    overviewLayout={overviewLayout}
                    onOverviewLayoutChange={setOverviewLayout}
                    themeMode={themeMode}
                    autoRefresh={data.autoRefresh}
                    onThemeModeChange={setThemeMode}
                    onAutoRefreshChange={data.setAutoRefresh}
                  />
                ) : null}
              </Suspense>
            </ErrorBoundary>
          </LoadingOverlay>
        </main>
      </div>
    </div>
  );
}
