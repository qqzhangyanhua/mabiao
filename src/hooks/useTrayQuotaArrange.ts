import { useCallback, useEffect, useRef, useState } from "react";
import {
  isTrayQuotaCollapsed,
  loadTrayQuotaLayout,
  persistTrayQuotaMove,
  quotaProviderFromPoint,
  saveTrayQuotaLayout,
  sortQuotaRows,
  toggleTrayQuotaCollapsed,
  TRAY_QUOTA_DRAG_THRESHOLD,
  type TrayQuotaLayout,
} from "../lib/trayQuotaLayout";
import type { OfficialQuotaRow } from "../types";

type DragSession = {
  from: string;
  startY: number;
  moved: boolean;
};

export function useTrayQuotaArrange(
  rows: OfficialQuotaRow[],
  enabled: boolean,
  onArrange?: () => void,
) {
  const [layout, setLayout] = useState(loadTrayQuotaLayout);
  const [dragging, setDragging] = useState<string | null>(null);
  const [dropTarget, setDropTarget] = useState<string | null>(null);
  const dragRef = useRef<DragSession | null>(null);
  const ignoreClickRef = useRef(false);
  const layoutRef = useRef(layout);
  const rowsRef = useRef(rows);
  const onArrangeRef = useRef(onArrange);

  useEffect(() => {
    layoutRef.current = layout;
    rowsRef.current = rows;
    onArrangeRef.current = onArrange;
  }, [layout, rows, onArrange]);

  const visible = enabled ? sortQuotaRows(rows, layout.order) : rows;

  const persist = useCallback((next: TrayQuotaLayout) => {
    setLayout(next);
    saveTrayQuotaLayout(next);
    onArrangeRef.current?.();
  }, []);

  const toggle = useCallback(
    (provider: string) => {
      if (ignoreClickRef.current) {
        ignoreClickRef.current = false;
        return;
      }
      persist({
        ...layoutRef.current,
        collapsed: toggleTrayQuotaCollapsed(layoutRef.current.collapsed, provider),
      });
    },
    [persist],
  );

  const beginDrag = useCallback((provider: string, clientY: number) => {
    if (!enabled) {
      return;
    }
    dragRef.current = { from: provider, startY: clientY, moved: false };
  }, [enabled]);

  useEffect(() => {
    if (!enabled) {
      return;
    }

    function finish(event: PointerEvent) {
      const drag = dragRef.current;
      dragRef.current = null;
      setDragging(null);
      setDropTarget(null);
      if (!drag) {
        return;
      }
      if (!drag.moved) {
        return;
      }
      ignoreClickRef.current = true;
      const over = quotaProviderFromPoint(event.clientX, event.clientY);
      if (!over || over === drag.from) {
        return;
      }
      const current = layoutRef.current;
      persist({
        ...current,
        order: persistTrayQuotaMove(
          current.order,
          rowsRef.current.map((row) => row.provider),
          drag.from,
          over,
        ),
      });
    }

    function onMove(event: PointerEvent) {
      const drag = dragRef.current;
      if (!drag) {
        return;
      }
      if (!drag.moved && Math.abs(event.clientY - drag.startY) >= TRAY_QUOTA_DRAG_THRESHOLD) {
        drag.moved = true;
        setDragging(drag.from);
      }
      if (!drag.moved) {
        return;
      }
      const over = quotaProviderFromPoint(event.clientX, event.clientY);
      setDropTarget(over && over !== drag.from ? over : null);
    }

    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", finish);
    window.addEventListener("pointercancel", finish);
    return () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", finish);
      window.removeEventListener("pointercancel", finish);
    };
  }, [enabled, persist]);

  return {
    visible,
    dragging,
    dropTarget,
    isCollapsed: (provider: string) => isTrayQuotaCollapsed(layout.collapsed, provider),
    toggle,
    beginDrag,
  };
}
