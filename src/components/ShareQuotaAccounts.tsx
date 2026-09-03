import type { OfficialQuotaRow } from "../types";

export function ShareQuotaAccounts({
  rows,
  selectedProvider,
  disabled,
  onSelect,
}: {
  rows: OfficialQuotaRow[];
  selectedProvider: string | null;
  disabled: boolean;
  onSelect: (provider: string) => void;
}) {
  if (rows.length <= 1) {
    return null;
  }
  return (
    <div className="report-dialog-accounts" role="listbox" aria-label="额度账号">
      {rows.map((row) => {
        const active = row.provider === selectedProvider;
        return (
          <button
            key={row.provider}
            type="button"
            role="option"
            aria-selected={active}
            className={active ? "report-dialog-account is-active" : "report-dialog-account"}
            disabled={disabled}
            onClick={() => onSelect(row.provider)}
          >
            <span className="report-dialog-account-name">{row.application}</span>
            {row.plan ? <span className="report-dialog-account-plan">{row.plan}</span> : null}
          </button>
        );
      })}
    </div>
  );
}
