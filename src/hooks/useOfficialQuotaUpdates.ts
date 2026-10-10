import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";
import type { OfficialQuotaDto } from "../types";

/** 后端整体刷新时每落库一家就广播一次全量快照，收到即替换，界面逐行变新。 */
export const OFFICIAL_QUOTA_UPDATED_EVENT = "official-quota-updated";

/** `onQuota` 要是稳定引用（state setter），否则每次渲染都会重新订阅。 */
export function useOfficialQuotaUpdates(onQuota: (quota: OfficialQuotaDto) => void): void {
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<OfficialQuotaDto>(OFFICIAL_QUOTA_UPDATED_EVENT, (event) => {
      onQuota(event.payload);
    }).then((fn) => {
      if (disposed) {
        fn();
        return;
      }
      unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [onQuota]);
}
