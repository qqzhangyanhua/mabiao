import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from "react";

export interface ExportAction {
  label: string;
  disabled: boolean;
  onClick: () => void;
}

interface ExportValue {
  actions: ExportAction[];
  setActions: (actions: ExportAction[]) => void;
}

const ExportContext = createContext<ExportValue | null>(null);

export function ExportProvider({ children }: { children: ReactNode }) {
  const [actions, setActions] = useState<ExportAction[]>([]);
  const value = useMemo(() => ({ actions, setActions }), [actions]);
  return <ExportContext.Provider value={value}>{children}</ExportContext.Provider>;
}

export function useExportBar(): ExportValue {
  const value = useContext(ExportContext);
  if (!value) throw new Error("useExportBar 只能在登录后的壳里用");
  return value;
}

/** 页面挂上导出按钮，离开时清掉。`actions` 须 `useMemo`。 */
export function usePageExports(actions: ExportAction[]): void {
  const { setActions } = useExportBar();
  useEffect(() => {
    setActions(actions);
    return () => setActions([]);
  }, [actions, setActions]);
}
