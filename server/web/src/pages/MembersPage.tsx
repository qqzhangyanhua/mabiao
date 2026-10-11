import { useCallback, useState, type FormEvent } from "react";
import type { AccountView, CoverageView } from "../api/types";
import { useAuth } from "../auth";
import { Empty, ErrorNote, Loading } from "../components/Feedback";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Card, panelClass } from "../components/ui/card";
import { Input } from "../components/ui/input";
import { useAsync } from "../hooks";
import { cn } from "../lib/cn";
import { coverageStatus } from "../lib/coverage";
import { downloadCsv } from "../lib/csv";
import { memberRows } from "../lib/export";
import { formatDateTime } from "../lib/format";
import { routeHash } from "../lib/route";

function CreateMemberForm({ onCreated }: { onCreated: () => void }) {
  const { api } = useAuth();
  const [account, setAccount] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await api.createMember(account.trim(), password);
      setAccount("");
      setPassword("");
      onCreated();
    } catch (e) {
      setError(e instanceof Error ? e.message : "创建失败");
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card className="p-4">
      <form onSubmit={submit} className="flex flex-wrap items-end gap-3 text-sm">
        <label>
          <span className="block text-muted-foreground">新成员账号</span>
          <Input
            value={account}
            onChange={(e) => setAccount(e.target.value)}
            required
            className="mt-1 w-52"
          />
        </label>
        <label>
          <span className="block text-muted-foreground">初始密码</span>
          <Input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            autoComplete="new-password"
            required
            className="mt-1 w-52"
          />
        </label>
        <Button type="submit" disabled={busy}>
          创建成员
        </Button>
        {error && (
          <p role="alert" className="w-full text-destructive">
            {error}
          </p>
        )}
      </form>
    </Card>
  );
}

function CoverageCell({
  account,
  coverage,
}: {
  account: AccountView;
  coverage: CoverageView | undefined;
}) {
  const status = coverage ? coverageStatus(coverage, new Date()) : "never";
  const text = coverage?.covered_through ?? "从未推送";
  // 停用账号不再推送，缺口是预期的，不标红。
  if (!account.active || status === "ok") return <span>{text}</span>;
  return (
    <Badge variant="warning" title="数据落后太久，或从未推送">
      {text}
      {status === "stale" ? "（有缺口）" : ""}
    </Badge>
  );
}

/** 成员管理（管理员）：开户、停用，以及每个成员的最后推送时间与已覆盖到哪天。 */
export function MembersPage() {
  const { api, profile } = useAuth();
  const load = useCallback(async () => {
    const [accounts, coverage] = await Promise.all([api.accounts(), api.coverage()]);
    return { accounts, coverage };
  }, [api]);
  const { data, error, loading, reload } = useAsync(load);
  const [actionError, setActionError] = useState<string | null>(null);

  async function deactivate(account: AccountView) {
    if (!window.confirm(`停用 ${account.account}？停用后立即退出登录、不能再推送，已有数据保留。`))
      return;
    setActionError(null);
    try {
      await api.deactivate(account.id);
      reload();
    } catch (e) {
      setActionError(e instanceof Error ? e.message : "停用失败");
    }
  }

  const coverageById = new Map(data?.coverage.map((c) => [c.account_id, c]));

  return (
    <>
      <div className="flex items-center justify-between gap-3">
        <h1 className="text-lg font-semibold">成员</h1>
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={!data}
          onClick={() => data && downloadCsv("成员.csv", memberRows(data.accounts, data.coverage))}
        >
          导出 CSV
        </Button>
      </div>
      <CreateMemberForm onCreated={reload} />
      {actionError && <ErrorNote error={new Error(actionError)} />}
      {error ? (
        <ErrorNote error={error} onRetry={reload} />
      ) : loading || !data ? (
        <Loading />
      ) : data.accounts.length === 0 ? (
        <Empty>还没有账号</Empty>
      ) : (
        <div className={cn(panelClass, "overflow-x-auto")}>
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border text-left text-xs text-muted-foreground">
                <th className="px-4 py-2 font-normal">账号</th>
                <th className="px-4 py-2 font-normal">角色</th>
                <th className="px-4 py-2 font-normal">状态</th>
                <th className="px-4 py-2 text-right font-normal">设备</th>
                <th className="px-4 py-2 font-normal">最后推送时间</th>
                <th className="px-4 py-2 font-normal">已覆盖到</th>
                <th className="px-4 py-2" />
              </tr>
            </thead>
            <tbody>
              {data.accounts.map((account) => {
                const coverage = coverageById.get(account.id);
                return (
                  <tr key={account.id} className="border-b border-border/70 last:border-0">
                    <td className="px-4 py-2">
                      <a
                        href={routeHash({ page: "member", accountId: account.id })}
                        className="text-primary hover:underline"
                      >
                        {account.account}
                      </a>
                    </td>
                    <td className="px-4 py-2">{account.role === "admin" ? "管理员" : "成员"}</td>
                    <td className="px-4 py-2">
                      {account.active ? (
                        "正常"
                      ) : (
                        <span className="text-muted-foreground">已停用</span>
                      )}
                    </td>
                    <td className="px-4 py-2 text-right tabular-nums">
                      {coverage?.device_count ?? 0}
                    </td>
                    <td className="px-4 py-2">{formatDateTime(coverage?.last_push_at ?? null)}</td>
                    <td className="px-4 py-2">
                      <CoverageCell account={account} coverage={coverage} />
                    </td>
                    <td className="px-4 py-2 text-right">
                      {account.active && account.id !== profile.id && (
                        <button
                          type="button"
                          onClick={() => void deactivate(account)}
                          className="text-destructive hover:underline"
                        >
                          停用
                        </button>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}
