import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { formatTokens, humanStatus } from "../lib/format";
import { prefillCandidatePrice, priceRowKey } from "../lib/priceCandidate";
import { formatPerMillionInput } from "../lib/priceUnits";
import type { PriceEntry, PriceTable, UnpricedGroupDto } from "../types";
import { EmptyState } from "./EmptyState";
import { SourceLabel } from "./SourceIcon";
import { Button } from "./ui/Button";

function providerLabel(provider: string): string {
  return provider || "（未标注）";
}

function formatCalibers(entry: PriceEntry): string {
  return [
    `输入 $${formatPerMillionInput(entry.input)}`,
    `输出 $${formatPerMillionInput(entry.output)}`,
    `缓存读 $${formatPerMillionInput(entry.cache_read)}`,
    `缓存写 $${formatPerMillionInput(entry.cache_creation)}`,
  ].join(" / ");
}

function CandidateCell({
  group,
  onPrefill,
}: {
  group: UnpricedGroupDto;
  onPrefill: (group: UnpricedGroupDto, candidate: PriceEntry) => void;
}) {
  const candidate = group.candidate;
  if (!candidate) {
    return <p className="unpriced-candidate-miss">快照里没有签名兼容的条目，可在下方价目表手填。</p>;
  }
  const fromSnapshot = candidate.origin !== "user";
  return (
    <div className="unpriced-candidate">
      <p className="unpriced-candidate-match">
        匹配 {candidate.model}
        {candidate.model !== group.model ? "（名称不同）" : null}
      </p>
      <p className={fromSnapshot ? "unpriced-candidate-origin" : "unpriced-candidate-origin user"}>
        {fromSnapshot
          ? "来自 LiteLLM 价目快照，不是你自己配的单价"
          : "来自已有用户价目的签名兼容条目，不是精确匹配"}
      </p>
      <p className="unpriced-candidate-price">{formatCalibers(candidate)} 每百万 Token</p>
      <p className="panel-note tone-warn unpriced-candidate-warn">
        签名匹配可能对上 flavor 不同的邻居，单价往往不一样。请核对后再保存。
      </p>
      <Button
        size="sm"
        onClick={() => {
          onPrefill(group, candidate);
        }}
      >
        预填到价目表
      </Button>
    </div>
  );
}

function GroupTable({
  rows,
  showModel,
  showCandidate,
  onPrefill,
}: {
  rows: UnpricedGroupDto[];
  showModel: boolean;
  showCandidate: boolean;
  onPrefill?: (group: UnpricedGroupDto, candidate: PriceEntry) => void;
}) {
  return (
    <div className="table-scroll">
      <table>
        <thead>
          <tr>
            {showModel ? <th>模型</th> : null}
            <th>接口</th>
            <th>来源</th>
            <th>Token</th>
            <th>记录</th>
            {showCandidate ? <th>快照候选</th> : null}
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
              {showCandidate && onPrefill ? (
                <td>
                  <CandidateCell group={row} onPrefill={onPrefill} />
                </td>
              ) : null}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function UnpricedDiagnosisPanel({
  prices,
  onChange,
  onPrefillHighlight,
}: {
  prices: PriceTable;
  onChange: (prices: PriceTable) => void;
  onPrefillHighlight: (key: string) => void;
}) {
  const [groups, setGroups] = useState<UnpricedGroupDto[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [prefillNote, setPrefillNote] = useState<string | null>(null);

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

  function prefill(group: UnpricedGroupDto, candidate: PriceEntry) {
    onChange({
      prices: prefillCandidatePrice(prices.prices, group, candidate),
    });
    onPrefillHighlight(priceRowKey(group.model, group.provider || null));
    setPrefillNote(
      `已把 ${group.model} 的四个口径预填进下方价目表，尚未写入价目文件。请核对后点保存。`,
    );
    document.getElementById("settings-prices")?.scrollIntoView({ block: "start" });
  }

  return (
    <section className="panel" id="settings-unpriced">
      <div className="panel-head">
        <div>
          <h2>未定价诊断</h2>
          <p className="panel-note">
            全库范围，不跟随分析页的时间、来源、模型或项目筛选，因此数量可能与拆分页「单价未配置」KPI
            不一致。补单价是一次性全局配置；可补区若有 LiteLLM 价目快照的签名候选，点一下只预填表单，确认保存后才会写入。
          </p>
        </div>
      </div>
      {error ? (
        <p className="panel-note tone-danger" role="alert">
          {error}
        </p>
      ) : null}
      {prefillNote ? <p className="panel-note tone-ok">{prefillNote}</p> : null}
      {groups === null && !error ? <p className="panel-note">正在读取全库未定价分组…</p> : null}

      <div className="unpriced-zone">
        <h3>可补单价</h3>
        <p className="panel-note">
          模型名非空，但精确查价未命中。候选来自 LiteLLM 价目快照，不是你自己配的单价；
          每一条都要单独预填并确认，没有「全部套用」。手填后保存，这一区可以清零。
        </p>
        {groups !== null && pricable.length === 0 ? (
          <EmptyState
            compact
            icon="check"
            title="可补单价已全部配齐"
            hint="有模型名的消耗记录现在都能算出费用。"
          />
        ) : null}
        {pricable.length > 0 ? (
          <GroupTable rows={pricable} showModel showCandidate onPrefill={prefill} />
        ) : null}
      </div>

      <div className="unpriced-zone">
        <h3>结构上无法计费</h3>
        <p className="panel-note">
          本机消耗记录没有模型名（Factory / droid 就是这种）。价目表以模型名为键，没有按
          接口计价的路径，补单价也算不出费用。
        </p>
        {groups !== null && structural.length === 0 ? (
          <p className="panel-note">当前没有无模型名的消耗记录。</p>
        ) : null}
        {structural.length > 0 ? (
          <GroupTable rows={structural} showModel={false} showCandidate={false} />
        ) : null}
      </div>
    </section>
  );
}
