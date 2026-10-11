import { useCallback, useState, type FormEvent } from "react";
import { useAuth } from "../auth";
import { Empty, ErrorNote, Loading } from "../components/Feedback";
import { Button } from "../components/ui/button";
import { Card, panelClass } from "../components/ui/card";
import { Input } from "../components/ui/input";
import { useAsync } from "../hooks";
import { cn } from "../lib/cn";
import { formatDateTime } from "../lib/format";

export function PricingPage() {
  const { api } = useAuth();
  const load = useCallback(() => api.pricing(), [api]);
  const { data, error, loading, reload } = useAsync(load);
  const [model, setModel] = useState("");
  const [provider, setProvider] = useState("");
  const [input, setInput] = useState("");
  const [output, setOutput] = useState("");
  const [cacheRead, setCacheRead] = useState("0");
  const [cacheCreation, setCacheCreation] = useState("0");
  const [busy, setBusy] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  async function save(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setFormError(null);
    setNotice(null);
    try {
      const result = await api.setPrice({
        model: model.trim(),
        provider: provider.trim() || undefined,
        input: Number(input),
        output: Number(output),
        cache_read: Number(cacheRead),
        cache_creation: Number(cacheCreation),
      });
      setNotice(`已写入价目，重算了 ${result.recomputed} 条消耗记录`);
      setModel("");
      setProvider("");
      setInput("");
      setOutput("");
      reload();
    } catch (e) {
      setFormError(e instanceof Error ? e.message : "保存失败");
    } finally {
      setBusy(false);
    }
  }

  async function remove(id: number) {
    if (!window.confirm("删除这条团队价目？同名模型会回落到内置快照。")) return;
    setFormError(null);
    try {
      const result = await api.deletePrice(id);
      setNotice(`已删除，重算了 ${result.recomputed} 条`);
      reload();
    } catch (e) {
      setFormError(e instanceof Error ? e.message : "删除失败");
    }
  }

  async function recompute() {
    setBusy(true);
    setFormError(null);
    try {
      const result = await api.recomputePricing();
      setNotice(`已全量重算 ${result.recomputed} 条`);
    } catch (e) {
      setFormError(e instanceof Error ? e.message : "重算失败");
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h1 className="text-lg font-semibold">团队价目</h1>
        <Button type="button" variant="outline" size="sm" disabled={busy} onClick={() => void recompute()}>
          全量重算
        </Button>
      </div>
      <p className="text-sm text-muted-foreground">
        跨人对比费用看统一价：来源自带费用优先，其次团队价目，再是内置 LiteLLM 快照。单价是每
        token 的价格，不要填「每百万 token」。
      </p>
      {notice && (
        <p role="status" className="text-sm text-success">
          {notice}
        </p>
      )}
      {formError && <ErrorNote error={new Error(formError)} />}
      <Card className="p-4">
        <form onSubmit={save} className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
          <label className="text-sm">
            模型
            <Input value={model} onChange={(e) => setModel(e.target.value)} required className="mt-1" />
          </label>
          <label className="text-sm">
            提供商（可空）
            <Input value={provider} onChange={(e) => setProvider(e.target.value)} className="mt-1" />
          </label>
          <label className="text-sm">
            输入单价
            <Input value={input} onChange={(e) => setInput(e.target.value)} required className="mt-1" />
          </label>
          <label className="text-sm">
            输出单价
            <Input value={output} onChange={(e) => setOutput(e.target.value)} required className="mt-1" />
          </label>
          <label className="text-sm">
            缓存读单价
            <Input value={cacheRead} onChange={(e) => setCacheRead(e.target.value)} className="mt-1" />
          </label>
          <label className="text-sm">
            缓存写单价
            <Input
              value={cacheCreation}
              onChange={(e) => setCacheCreation(e.target.value)}
              className="mt-1"
            />
          </label>
          <div className="flex items-end">
            <Button type="submit" disabled={busy}>
              保存价目
            </Button>
          </div>
        </form>
      </Card>
      {error ? (
        <ErrorNote error={error} onRetry={reload} />
      ) : loading || !data ? (
        <Loading />
      ) : (
        <>
          <p className="text-xs text-muted-foreground">
            内置快照 {data.snapshot.source} · {data.snapshot.as_of} · {data.snapshot.count} 条
          </p>
          {data.prices.length === 0 ? (
            <Empty>还没有团队价目，统一费用会走内置快照</Empty>
          ) : (
            <div className={cn(panelClass, "overflow-x-auto")}>
              <table className="w-full text-sm">
                <caption className="sr-only">团队价目</caption>
                <thead>
                  <tr className="border-b border-border text-left text-xs text-muted-foreground">
                    <th scope="col" className="px-4 py-2 font-normal">
                      模型
                    </th>
                    <th scope="col" className="px-4 py-2 font-normal">
                      提供商
                    </th>
                    <th scope="col" className="px-4 py-2 text-right font-normal">
                      输入
                    </th>
                    <th scope="col" className="px-4 py-2 text-right font-normal">
                      输出
                    </th>
                    <th scope="col" className="px-4 py-2 text-right font-normal">
                      缓存读
                    </th>
                    <th scope="col" className="px-4 py-2 text-right font-normal">
                      缓存写
                    </th>
                    <th scope="col" className="px-4 py-2 font-normal">
                      更新
                    </th>
                    <th scope="col" className="px-4 py-2" />
                  </tr>
                </thead>
                <tbody>
                  {data.prices.map((price) => (
                    <tr key={price.id} className="border-b border-border/70 last:border-0">
                      <th scope="row" className="px-4 py-2 font-normal">
                        {price.model}
                      </th>
                      <td className="px-4 py-2">{price.provider ?? "—"}</td>
                      <td className="px-4 py-2 text-right tabular-nums">{price.input}</td>
                      <td className="px-4 py-2 text-right tabular-nums">{price.output}</td>
                      <td className="px-4 py-2 text-right tabular-nums">{price.cache_read}</td>
                      <td className="px-4 py-2 text-right tabular-nums">{price.cache_creation}</td>
                      <td className="px-4 py-2">{formatDateTime(price.updated_at)}</td>
                      <td className="px-4 py-2 text-right">
                        <button
                          type="button"
                          onClick={() => void remove(price.id)}
                          className="text-destructive hover:underline"
                        >
                          删除
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </>
      )}
    </>
  );
}
