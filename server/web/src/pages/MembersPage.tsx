import { useCallback, useMemo, useState, type FormEvent } from "react";
import type { AccountView, CoverageView } from "../api/types";
import { useAuth } from "../auth";
import { Empty, ErrorNote, Loading } from "../components/Feedback";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Card, panelClass } from "../components/ui/card";
import { Input } from "../components/ui/input";
import { Segmented } from "../components/ui/segmented";
import { usePageExports } from "../export-action";
import { useAsync } from "../hooks";
import { cn } from "../lib/cn";
import { costOf } from "../lib/cost";
import { coverageStatus } from "../lib/coverage";
import { downloadCsv } from "../lib/csv";
import { memberRows } from "../lib/export";
import { formatCost, formatRelativeDay } from "../lib/format";
import { localOffsetMinutes, presetRange, queryBounds } from "../lib/range";
import { useHref, useView } from "../view";

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
  if (!account.active || status === "ok") return <span>{text}</span>;
  return (
    <Badge variant="warning" title="数据落后太久，或从未推送">
      {text}
      {status === "stale" ? "（有缺口）" : ""}
    </Badge>
  );
}

type RoleFilter = "all" | "admin" | "member";

export function MembersPage() {
  const { api, profile } = useAuth();
  const view = useView();
  const href = useHref();
  const load = useCallback(async () => {
    const [accounts, coverage] = await Promise.all([api.accounts(), api.coverage()]);
    return { accounts, coverage };
  }, [api]);
  const { data, error, loading, reload } = useAsync(load);
  const weekBounds = useMemo(() => {
    const now = new Date();
    const offset = localOffsetMinutes(now);
    return queryBounds(presetRange("7d", now, offset), offset);
  }, []);
  const loadWeek = useCallback(
    () => (weekBounds ? api.summary(weekBounds, localOffsetMinutes()) : Promise.reject(new Error("日期范围不正确"))),
    [api, weekBounds],
  );
  const week = useAsync(loadWeek);
  const [actionError, setActionError] = useState<string | null>(null);
  const [roleFilter, setRoleFilter] = useState<RoleFilter>("all");
  const [search, setSearch] = useState("");

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
  const spendById = new Map((week.data?.by_account ?? []).map((row) => [row.id, costOf(row, view.mode)]));
  const weekSpend = week.data
    ? view.mode === "unified"
      ? week.data.totals.unified_cost_total
      : week.data.totals.cost_snapshot_total
    : 0;
  const weekFromMs = weekBounds ? Date.parse(weekBounds.from) : 0;
  const activeThisWeek = (data?.coverage ?? []).filter((row) => {
    if (!row.last_push_at) return false;
    return Date.parse(row.last_push_at) >= weekFromMs;
  }).length;

  const filtered = (data?.accounts ?? []).filter((account) => {
    if (roleFilter !== "all" && account.role !== roleFilter) return false;
    if (search.trim() && !account.account.toLowerCase().includes(search.trim().toLowerCase()))
      return false;
    return true;
  });

  usePageExports(
    useMemo(
      () => [
        {
          label: "导出 CSV",
          disabled: !data,
          onClick: () => data && downloadCsv("成员.csv", memberRows(data.accounts, data.coverage)),
        },
      ],
      [data],
    ),
  );

  return (
    <>
      <h1 className="text-lg font-semibold">成员</h1>
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
        <div className={cn(panelClass, "p-4")}>
          <div className="text-xs text-muted-foreground">成员</div>
          <div className="mt-1 text-2xl font-semibold tabular-nums">{data?.accounts.length ?? "…"}</div>
        </div>
        <div className={cn(panelClass, "p-4")}>
          <div className="text-xs text-muted-foreground">本周活跃</div>
          <div className="mt-1 text-2xl font-semibold tabular-nums">{data ? activeThisWeek : "…"}</div>
          <p className="mt-1 text-xs text-muted-foreground">近 7 天有推送</p>
        </div>
        <div className={cn(panelClass, "p-4")}>
          <div className="text-xs text-muted-foreground">本周费用</div>
          <div className="mt-1 text-2xl font-semibold tabular-nums">
            {week.data ? formatCost(weekSpend) : "…"}
          </div>
        </div>
      </div>
      <CreateMemberForm onCreated={reload} />
      {actionError && <ErrorNote error={new Error(actionError)} />}
      <div className="flex flex-wrap items-center gap-3">
        <Segmented
          label="角色"
          value={roleFilter}
          onChange={setRoleFilter}
          options={[
            { id: "all", label: "全部" },
            { id: "admin", label: "管理员" },
            { id: "member", label: "成员" },
          ]}
        />
        <Input
          aria-label="搜索账号"
          placeholder="搜索账号"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className="max-w-xs"
        />
      </div>
      {error ? (
        <ErrorNote error={error} onRetry={reload} />
      ) : loading || !data ? (
        <Loading />
      ) : filtered.length === 0 ? (
        <Empty>没有匹配的账号</Empty>
      ) : (
        <div className={cn(panelClass, "overflow-x-auto")}>
          <table className="w-full text-sm">
            <caption className="sr-only">远程账号与推送覆盖</caption>
            <thead>
              <tr className="border-b border-border text-left text-xs text-muted-foreground">
                <th scope="col" className="px-4 py-2 font-normal">
                  账号
                </th>
                <th scope="col" className="px-4 py-2 font-normal">
                  角色
                </th>
                <th scope="col" className="px-4 py-2 font-normal">
                  状态
                </th>
                <th scope="col" className="px-4 py-2 font-normal">
                  最后活动
                </th>
                <th scope="col" className="px-4 py-2 text-right font-normal">
                  本周费用
                </th>
                <th scope="col" className="px-4 py-2 text-right font-normal">
                  设备
                </th>
                <th scope="col" className="px-4 py-2 font-normal">
                  已覆盖到
                </th>
                <th scope="col" className="px-4 py-2" />
              </tr>
            </thead>
            <tbody>
              {filtered.map((account) => {
                const coverage = coverageById.get(account.id);
                return (
                  <tr key={account.id} className="border-b border-border/70 last:border-0">
                    <th scope="row" className="px-4 py-2 font-normal">
                      <a
                        href={href({ page: "member", accountId: account.id })}
                        className="text-primary hover:underline"
                      >
                        {account.account}
                      </a>
                    </th>
                    <td className="px-4 py-2">{account.role === "admin" ? "管理员" : "成员"}</td>
                    <td className="px-4 py-2">
                      {account.active ? "正常" : <span className="text-muted-foreground">已停用</span>}
                    </td>
                    <td className="px-4 py-2">{formatRelativeDay(coverage?.last_push_at ?? null)}</td>
                    <td className="px-4 py-2 text-right tabular-nums">
                      {formatCost(spendById.get(account.id) ?? 0)}
                    </td>
                    <td className="px-4 py-2 text-right tabular-nums">{coverage?.device_count ?? 0}</td>
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
