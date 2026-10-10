import { useCallback, useState } from "react";
import type { Project } from "../api/types";
import { useAuth } from "../auth";
import { useAsync } from "../hooks";
import { routeHash } from "../lib/route";
import { ErrorNote } from "./Feedback";

/**
 * 管理员手动改名、合并（覆盖 git remote / 目录名的自动归并）。
 * 合并不可撤销：历史会话与消耗记录改挂目标，之后同一个 remote 的推送也归到目标。
 */
export function ProjectAdmin({ project, onRenamed }: { project: Project; onRenamed: () => void }) {
  const { api } = useAuth();
  const [name, setName] = useState(project.name);
  const [targetId, setTargetId] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<Error | undefined>();
  const loadProjects = useCallback(() => api.adminProjects(), [api]);
  const projects = useAsync(loadProjects);
  const targets = (projects.data ?? []).filter((p) => p.id !== project.id);
  const target = targets.find((p) => String(p.id) === targetId);

  async function run(action: () => Promise<void>) {
    setBusy(true);
    setError(undefined);
    try {
      await action();
    } catch (e) {
      setError(e instanceof Error ? e : new Error(String(e)));
    } finally {
      setBusy(false);
    }
  }

  const rename = () =>
    run(async () => {
      await api.renameProject(project.id, name);
      onRenamed();
      projects.reload();
    });

  const merge = () =>
    run(async () => {
      if (!target) return;
      const ok = window.confirm(
        `把「${project.name}」合并进「${target.name}」？\n历史会话与消耗记录都会改挂到「${target.name}」，不能撤销。`,
      );
      if (!ok) return;
      const result = await api.mergeProject(project.id, target.id);
      window.location.hash = routeHash({ page: "project", projectId: result.project.id });
    });

  return (
    <section className="space-y-4 rounded border border-slate-200 bg-white p-4">
      <h2 className="text-sm font-medium text-slate-700">项目管理（管理员）</h2>
      <p className="text-xs text-slate-500">
        自动归并的依据：{project.git_remote ? `git remote ${project.git_remote}` : "没有 remote，按目录名"}
        。认错了可以手动改名或合并。
      </p>
      {error && <ErrorNote error={error} />}
      <div className="flex flex-wrap items-end gap-2 text-sm">
        <label className="flex flex-col gap-1">
          <span className="text-xs text-slate-500">项目名</span>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            maxLength={200}
            className="w-64 rounded border border-slate-300 px-2 py-1"
          />
        </label>
        <button
          type="button"
          disabled={busy || name.trim() === "" || name.trim() === project.name}
          onClick={() => void rename()}
          className="rounded border border-slate-300 px-3 py-1 hover:bg-slate-100 disabled:opacity-50"
        >
          改名
        </button>
      </div>
      <div className="flex flex-wrap items-end gap-2 text-sm">
        <label className="flex flex-col gap-1">
          <span className="text-xs text-slate-500">合并进</span>
          <select
            value={targetId}
            onChange={(e) => setTargetId(e.target.value)}
            className="w-64 rounded border border-slate-300 px-2 py-1"
          >
            <option value="">选择目标项目…</option>
            {targets.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}（{p.session_count} 场 / {p.usage_record_count} 条记录）
              </option>
            ))}
          </select>
        </label>
        <button
          type="button"
          disabled={busy || !target}
          onClick={() => void merge()}
          className="rounded border border-red-300 px-3 py-1 text-red-700 hover:bg-red-50 disabled:opacity-50"
        >
          合并
        </button>
      </div>
    </section>
  );
}
