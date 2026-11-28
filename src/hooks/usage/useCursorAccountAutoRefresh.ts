import { useEffect, useRef, useState } from "react";
import {
  CURSOR_ACCOUNT_AUTO_REFRESH_MINUTES,
  CURSOR_ACCOUNT_AUTO_REFRESH_STORAGE_KEY,
  loadCursorAccountAutoRefresh,
} from "./constants";

/**
 * ADR-0006 独立的 Cursor 账号用量自动刷新定时器：
 * 默认关闭，固定每 10 分钟联网拉取，独立于本机会话的 1/5/10 分钟摄取定时器。
 * 视图刷新与请求代次保护全部交回 `UsageViewController`，此处不再维护第二套视图刷新表。
 */
export function useCursorAccountAutoRefresh(refreshCursorAccount: () => Promise<void>) {
  const [autoRefresh, setAutoRefresh] = useState(loadCursorAccountAutoRefresh);
  const refreshRef = useRef(refreshCursorAccount);
  const wasEnabledRef = useRef(autoRefresh);

  useEffect(() => {
    refreshRef.current = refreshCursorAccount;
  }, [refreshCursorAccount]);

  useEffect(() => {
    try {
      window.localStorage.setItem(
        CURSOR_ACCOUNT_AUTO_REFRESH_STORAGE_KEY,
        autoRefresh ? "on" : "off",
      );
    } catch {
      // localStorage 不可用时忽略，仅影响下次启动是否记住选择
    }
    if (!autoRefresh) {
      return;
    }
    const id = window.setInterval(() => {
      void refreshRef.current().catch(() => undefined);
    }, CURSOR_ACCOUNT_AUTO_REFRESH_MINUTES * 60_000);
    return () => window.clearInterval(id);
  }, [autoRefresh]);

  useEffect(() => {
    const wasEnabled = wasEnabledRef.current;
    wasEnabledRef.current = autoRefresh;
    if (autoRefresh && !wasEnabled) {
      void refreshRef.current().catch(() => undefined);
    }
  }, [autoRefresh]);

  return { autoRefresh, setAutoRefresh };
}
