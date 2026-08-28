import { priceRowKey } from "../lib/priceCandidate";
import { formatPerMillionInput, parsePerMillionInput } from "../lib/priceUnits";
import type { PriceEntry, PriceTable } from "../types";
import { Button } from "./ui/Button";
import { Field } from "./ui/Field";

export function PriceConfigPanel({
  prices,
  highlightKey,
  onChange,
  onSave,
}: {
  prices: PriceTable;
  highlightKey?: string | null;
  onChange: (prices: PriceTable) => void;
  onSave: () => void;
}) {
  function update(index: number, patch: Partial<PriceEntry>) {
    const next = prices.prices.map((row, i) => (i === index ? { ...row, ...patch } : row));
    onChange({ prices: next });
  }

  return (
    <section className="panel" id="settings-prices">
      <div className="panel-head">
        <div>
          <h2>单价配置</h2>
          <p className="panel-note">单价按 USD / 1M Token 填写；保存后仍按每 Token 存储。</p>
        </div>
        <div className="row-actions">
          <Button
            onClick={() =>
              onChange({
                prices: [
                  ...prices.prices,
                  {
                    model: "",
                    provider: null,
                    input: 0,
                    output: 0,
                    cache_read: 0,
                    cache_creation: 0,
                  },
                ],
              })
            }
          >
            新增
          </Button>
          <Button variant="accent" onClick={onSave}>
            保存
          </Button>
        </div>
      </div>
      {prices.prices.map((row, index) => (
        <div
          className={
            highlightKey && priceRowKey(row.model, row.provider) === highlightKey
              ? "price-row price-row-prefilled"
              : "price-row"
          }
          key={index}
        >
          <Field
            label="模型"
            placeholder="模型名"
            value={row.model}
            onChange={(event) => update(index, { model: event.target.value })}
          />
          <Field
            label="Provider"
            placeholder="可空"
            value={row.provider ?? ""}
            onChange={(event) => update(index, { provider: event.target.value || null })}
          />
          <Field
            label="输入 / 1M"
            type="number"
            min="0"
            step="any"
            value={formatPerMillionInput(row.input)}
            onChange={(event) => {
              const parsed = parsePerMillionInput(event.target.value);
              if (parsed === null) {
                return;
              }
              update(index, { input: parsed });
            }}
          />
          <Field
            label="输出 / 1M"
            type="number"
            min="0"
            step="any"
            value={formatPerMillionInput(row.output)}
            onChange={(event) => {
              const parsed = parsePerMillionInput(event.target.value);
              if (parsed === null) {
                return;
              }
              update(index, { output: parsed });
            }}
          />
          <Field
            label="缓存读 / 1M"
            type="number"
            min="0"
            step="any"
            value={formatPerMillionInput(row.cache_read)}
            onChange={(event) => {
              const parsed = parsePerMillionInput(event.target.value);
              if (parsed === null) {
                return;
              }
              update(index, { cache_read: parsed });
            }}
          />
          <Field
            label="缓存写 / 1M"
            type="number"
            min="0"
            step="any"
            value={formatPerMillionInput(row.cache_creation)}
            onChange={(event) => {
              const parsed = parsePerMillionInput(event.target.value);
              if (parsed === null) {
                return;
              }
              update(index, { cache_creation: parsed });
            }}
          />
          <Button
            variant="danger"
            className="price-row-delete"
            onClick={() => onChange({ prices: prices.prices.filter((_, i) => i !== index) })}
          >
            删除
          </Button>
        </div>
      ))}
    </section>
  );
}
