import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { formatTokens, humanStatus } from "../lib/format";
import type { UnpricedGroupDto } from "../types";
import { EmptyState } from "./EmptyState";
import { SourceLabel } from "./SourceIcon";

function providerLabel(provider: string): string {
  return provider || "（未标注）";
}

function GroupTable({
  rows,
  showModel,
}: {
  rows: UnpricedGroupDto[];
  showModel: boolean;
}) {
  return (
    <div className="table-scroll">
      <table>
        <thead>
          <tr>
            {showModel ? <th>模型</th> : null}
            <th>Provider</th>
            <th>来源</th>
            <th>Token</th>
            <th>记录</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={`${row.model}\u001f${row.provider}`}>
              {showModel ? <td>{row.model}</td> : null}
              <td>{providerLabel(row.provider)}</td>
              <td>
                <div className="unpriced-sources">
                  {row.sources.map((source) => (
                    <SourceLabel key={source} source={source} size={14} />
                  ))}
                </div>
              </td>
              <td>{formatTokens(row.total_tokens)}</td>
              <td>{formatTokens(row.record_count)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function UnpricedDiagnosisPanel() {
  const [groups, setGroups] = useState<UnpricedGroupDto[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    invoke<UnpricedGroupDto[]>("get_unpriced_diagnosis")
      .then((rows) => {
        if (alive) {
          setGroups(rows);
        }
      })
      .catch((err: unknown) => {
        if (alive) {
          setError(humanStatus(err));
        }
      });
    return () => {
      alive = false;
    };
  }, []);

  const pricable = groups?.filter((row) => row.reason === "pricable") ?? [];
  const structural = groups?.filter((row) => row.reason === "structurally_unbillable") ?? [];

  return (
    <section className="panel" id="settings-unpriced">
      <div className="panel-head">
        <div>
          <h2>未定价诊断</h2>
          <p className="panel-note">
            全库范围，不跟随分析页的时间、来源、模型或项目筛选。补单价是一次性全局配置；
            在下方价目表手填并保存后，可补区会相应减少。
          </p>
        </div>
      </div>
      {error ? (
        <p className="panel-note tone-danger" role="alert">
          {error}
        </p>
      ) : null}
      {groups === null && !error ? <p className="panel-note">正在读取全库未定价分组…</p> : null}

      <div className="unpriced-zone">
        <h3>可补单价</h3>
        <p className="panel-note">
          模型名非空，但精确查价未命中。在下方价目表手填对应模型的单价并保存，这一区可以清零。
        </p>
        {groups !== null && pricable.length === 0 ? (
          <EmptyState
            compact
            icon="check"
            title="可补单价已全部配齐"
            hint="有模型名的消耗记录现在都能算出费用。"
          />
        ) : null}
        {pricable.length > 0 ? <GroupTable rows={pricable} showModel /> : null}
      </div>

      <div className="unpriced-zone">
        <h3>结构上无法计费</h3>
        <p className="panel-note">
          本机消耗记录没有模型名（Factory / droid 就是这种）。价目表以模型名为键，没有按
          Provider 计价的路径，补单价也算不出费用。
        </p>
        {groups !== null && structural.length === 0 ? (
          <p className="panel-note">当前没有无模型名的消耗记录。</p>
        ) : null}
        {structural.length > 0 ? <GroupTable rows={structural} showModel={false} /> : null}
      </div>
    </section>
  );
}
