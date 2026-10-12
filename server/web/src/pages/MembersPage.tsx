import { Activity, UserRound, Users, Wallet } from "lucide-react";
import { useCallback, useMemo, useState, type FormEvent } from "react";
import type { AccountView, CoverageView } from "../api/types";
import { useAuth } from "../auth";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { Empty, ErrorNote, Loading } from "../components/Feedback";
import { MetricCard } from "../components/KpiColumn";
import { PageHeader } from "../components/PageHeader";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Card, panelClass } from "../components/ui/card";
import { Input } from "../components/ui/input";
import { Segmented } from "../components/ui/segmented";
import { numCellClass, tableClass, thClass, theadRowClass, trClass } from "../components/ui/table";
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
  const [pendingDeactivate, setPendingDeactivate] = useState<AccountView | null>(null);
  const [deactivating, setDeactivating] = useState(false);

  async function deactivate(account: AccountView) {
    setDeactivating(true);
    setActionError(null);
    try {
      await api.deactivate(account.id);
      reload();
    } catch (e) {
      setActionError(e instanceof Error ? e.message : "停用失败");
    } finally {
      setDeactivating(false);
      setPendingDeactivate(null);
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
      <PageHeader
        icon={<UserRound size={22} strokeWidth={1.8} />}
        title="成员"
        subtitle="远程账号、推送覆盖与近 7 天费用"
      />
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
        <MetricCard
          value={data ? String(data.accounts.length) : "…"}
          label="成员"
          icon={<Users size={18} />}
          tone="violet"
        />
        <MetricCard
          value={data ? String(activeThisWeek) : "…"}
          label="本周活跃"
          hint="近 7 天有推送"
          icon={<Activity size={18} />}
          tone="emerald"
        />
        <MetricCard
          value={week.data ? formatCost(weekSpend) : "…"}
          label="本周费用"
          icon={<Wallet size={18} />}
          tone="amber"
        />
      </div>
      <CreateMemberForm onCreated={reload} />
      {actionError && <ErrorNote error={new Error(actionError)} />}
      <ConfirmDialog
        open={pendingDeactivate !== null}
        title={`停用 ${pendingDeactivate?.account ?? ""}？`}
        description="停用后立即退出登录、不能再推送，已有数据保留。"
        confirmLabel="确认停用"
        busyLabel="停用中…"
        busy={deactivating}
        onConfirm={() => pendingDeactivate && void deactivate(pendingDeactivate)}
        onCancel={() => setPendingDeactivate(null)}
      />
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
          <table className={tableClass}>
            <caption className="sr-only">远程账号与推送覆盖</caption>
            <thead>
              <tr className={theadRowClass}>
                <th scope="col" className={`px-4 py-2 ${thClass}`}>
                  账号
                </th>
                <th scope="col" className={`px-4 py-2 ${thClass}`}>
                  角色
                </th>
                <th scope="col" className={`px-4 py-2 ${thClass}`}>
                  状态
                </th>
                <th scope="col" className={`px-4 py-2 ${thClass}`}>
                  最后活动
                </th>
                <th scope="col" className={`px-4 py-2 ${numCellClass} ${thClass}`}>
                  本周费用
                </th>
                <th scope="col" className={`px-4 py-2 ${numCellClass} ${thClass}`}>
                  设备
                </th>
                <th scope="col" className={`px-4 py-2 ${thClass}`}>
                  已覆盖到
                </th>
                <th scope="col" className="px-4 py-2" />
              </tr>
            </thead>
            <tbody>
              {filtered.map((account) => {
                const coverage = coverageById.get(account.id);
                return (
                  <tr key={account.id} className={trClass}>
                    <th scope="row" className="px-4 py-2 text-left font-normal">
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
                    <td className={`px-4 py-2 ${numCellClass}`}>
                      {formatCost(spendById.get(account.id) ?? 0)}
                    </td>
                    <td className={`px-4 py-2 ${numCellClass}`}>{coverage?.device_count ?? 0}</td>
                    <td className="px-4 py-2">
                      <CoverageCell account={account} coverage={coverage} />
                    </td>
                    <td className="px-4 py-2 text-right">
                      {account.active && account.id !== profile.id && (
                        <button
                          type="button"
                          onClick={() => setPendingDeactivate(account)}
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
