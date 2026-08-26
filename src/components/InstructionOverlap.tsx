import { projectLabel } from "../lib/format";
import type { InstructionOverlapHint } from "../types";
import { EmptyState } from "./EmptyState";
import { Select } from "./ui/Select";

export function InstructionOverlap({
  selectedProject,
  projects,
  hints,
  onProjectChange,
}: {
  selectedProject: string | null;
  projects: string[];
  hints: InstructionOverlapHint[];
  onProjectChange: (project: string) => void;
}) {
  return (
    <section className="instruction-overlap">
      <header className="instruction-section-head">
        <div>
          <h3>与项目规则交叉</h3>
          <p className="muted">两侧出现相同关键词并不等于已经冲突，请对照原文自行判断。</p>
        </div>
        {projects.length > 0 ? (
          <Select
            value={selectedProject ?? ""}
            options={projects.map((path) => ({ value: path, label: projectLabel(path) }))}
            ariaLabel="比对项目"
            align="right"
            onChange={onProjectChange}
          />
        ) : null}
      </header>
      {projects.length === 0 ? (
        <EmptyState
          compact
          title="没有可比对的项目"
          hint="有消耗记录且目录还在的项目会出现在这里。"
        />
      ) : hints.length === 0 ? (
        <EmptyState
          compact
          title="没有共现关键词"
          hint="中文词按连续三字以上匹配，英文词至少四个字母。共现不等于已经冲突。"
        />
      ) : (
        <ul className="instruction-overlap-list">
          {hints.map((hint) => (
            <li
              key={`${hint.keyword}:${hint.global_display_path}:${hint.project_display_path}`}
              className="instruction-overlap-item"
            >
              <p className="instruction-overlap-keyword">
                两侧都提到了「{hint.keyword}」，请自行判断是否相互制约。
              </p>
              <div className="instruction-overlap-pair">
                <blockquote>
                  <span>
                    {hint.global_application} · {hint.global_display_path}
                  </span>
                  <p>{hint.global_snippet}</p>
                </blockquote>
                <blockquote>
                  <span>{hint.project_display_path}</span>
                  <p>{hint.project_snippet}</p>
                </blockquote>
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
