import type { Ref } from "react";
import type { QuotaCardViewModel } from "../lib/quotaCard";
import "./quotaPoster.css";

export function QuotaPoster({
  data,
  posterRef,
}: {
  data: QuotaCardViewModel;
  posterRef?: Ref<HTMLElement | null>;
}) {
  return (
    <article ref={posterRef} className="quota-poster">
      <p className="qp-kicker">{data.kicker}</p>
      <p className="qp-account">{data.accountLabel}</p>
      {data.planLabel ? <p className="qp-plan">{data.planLabel}</p> : null}
      <ul className="qp-windows">
        {data.windows.map((window, index) => (
          <li key={`${window.label}-${index}`} className="qp-window">
            <div className="qp-window-head">
              <span className="qp-window-label">{window.label}</span>
              {window.percentLabel || window.amountLabel ? (
                <span className="qp-window-value">{window.percentLabel ?? window.amountLabel}</span>
              ) : null}
            </div>
            {window.percent == null ? null : (
              <div className="qp-bar" aria-hidden="true">
                <i style={{ width: `${Math.min(100, Math.max(0, window.percent))}%` }} />
              </div>
            )}
            {window.percentLabel && window.amountLabel ? (
              <p className="qp-window-meta">{window.amountLabel}</p>
            ) : null}
            {window.resetLabel ? <p className="qp-window-meta">{window.resetLabel}</p> : null}
            {window.exhaustLabel ? (
              <p className="qp-window-exhaust">{window.exhaustLabel}</p>
            ) : null}
          </li>
        ))}
      </ul>
      <p className="qp-captured">{data.capturedAtLabel}</p>
    </article>
  );
}
