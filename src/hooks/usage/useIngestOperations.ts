import { invoke } from "@tauri-apps/api/core";
import { useCallback, useRef, useState, type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import { humanStatus } from "../../lib/format";
import { ingestFailureStatus, usageStageStatus } from "../../lib/ingestStatus";
import type { IngestReport } from "../../types";

type IngestOpsConfig = {
  refreshViews: () => Promise<void>;
  dataEpochRef: MutableRefObject<number>;
  requestGenerationRef: MutableRefObject<number>;
  setSessionsRevision: Dispatch<SetStateAction<number>>;
  setLastIngestReport: Dispatch<SetStateAction<IngestReport | null>>;
  setStatus: Dispatch<SetStateAction<string>>;
  setLoading: Dispatch<SetStateAction<boolean>>;
};

export function useIngestOperations({
  refreshViews,
  dataEpochRef,
  requestGenerationRef,
  setSessionsRevision,
  setLastIngestReport,
  setStatus,
  setLoading,
}: IngestOpsConfig) {
  const ingestOperation = useRef(false);
  const usageStatusRef = useRef("");
  const [busy, setBusy] = useState(false);
  const [rebuilding, setRebuilding] = useState<string | null>(null);
  const [purging, setPurging] = useState<string | null>(null);

  const runIngest = useCallback(
    async (label: string) => {
      if (ingestOperation.current) {
        return;
      }
      ingestOperation.current = true;
      requestGenerationRef.current += 1;
      setBusy(true);
      setStatus(`${label}中…`);
      try {
        const report = await invoke<IngestReport>("ingest");
        dataEpochRef.current += 1;
        setSessionsRevision((n) => n + 1);
        setLastIngestReport(report);
        const text = usageStageStatus(label, report);
        usageStatusRef.current = text;
        setStatus(text);
        await refreshViews();
        try {
          await invoke("refresh_tray");
        } catch {
          // 菜单栏刷新失败不阻断主界面
        }
      } catch (error) {
        setStatus(ingestFailureStatus(label, humanStatus(error)));
        setLoading(false);
      } finally {
        ingestOperation.current = false;
        setBusy(false);
      }
    },
    [
      refreshViews,
      dataEpochRef,
      requestGenerationRef,
      setSessionsRevision,
      setLastIngestReport,
      setStatus,
      setLoading,
    ],
  );

  const runRebuild = useCallback(
    async (source: string | null) => {
      if (ingestOperation.current) {
        return;
      }
      ingestOperation.current = true;
      requestGenerationRef.current += 1;
      const target = source ?? "all";
      setRebuilding(target);
      setBusy(true);
      setStatus(`${source ? `${source} ` : "全部"}缓存重建中…`);
      try {
        const report = await invoke<IngestReport>("rebuild_cache", { source });
        dataEpochRef.current += 1;
        setSessionsRevision((n) => n + 1);
        setLastIngestReport(report);
        const archived = report.records_archived > 0 ? `，归档 ${report.records_archived}` : "";
        setStatus(
          `缓存重建${report.partial_success ? "部分完成" : "完成"}：写入 ${report.records_written}${archived}，清理 ${report.records_removed}，失败 ${report.files_failed}`,
        );
        await refreshViews();
        try {
          await invoke("refresh_tray");
        } catch {
          // 菜单栏刷新失败不阻断主界面
        }
      } catch (error) {
        setStatus(`缓存重建失败：${humanStatus(error)}`);
        setLoading(false);
      } finally {
        ingestOperation.current = false;
        setRebuilding(null);
        setBusy(false);
      }
    },
    [
      refreshViews,
      dataEpochRef,
      requestGenerationRef,
      setSessionsRevision,
      setLastIngestReport,
      setStatus,
      setLoading,
    ],
  );

  const runPurgeArchived = useCallback(
    async (source: string | null) => {
      if (ingestOperation.current) {
        return;
      }
      ingestOperation.current = true;
      const target = source ?? "all";
      setPurging(target);
      setBusy(true);
      setStatus(`正在清理${source ? `${source} ` : "全部"}已归档记录…`);
      try {
        const removed = await invoke<number>("purge_archived_records", { source });
        dataEpochRef.current += 1;
        setSessionsRevision((n) => n + 1);
        setStatus(`已永久删除 ${removed} 条归档记录`);
        await refreshViews();
      } catch (error) {
        setStatus(`清理归档记录失败：${humanStatus(error)}`);
      } finally {
        ingestOperation.current = false;
        setPurging(null);
        setBusy(false);
      }
    },
    [refreshViews, dataEpochRef, setSessionsRevision, setStatus],
  );

  return { busy, rebuilding, purging, runIngest, runRebuild, runPurgeArchived, ingestOperation, usageStatusRef };
}
