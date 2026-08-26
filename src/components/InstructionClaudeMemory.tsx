import { useState } from "react";
import { formatBytes, formatClock, projectLabel } from "../lib/format";
import type { ClaudeAutoMemoryRepo } from "../types";

export function InstructionClaudeMemory({ repos }: { repos: ClaudeAutoMemoryRepo[] }) {
  const [openRepo, setOpenRepo] = useState<string | null>(null);
  if (repos.length === 0) {
    return null;
  }
  return (
    <section className="instruction-memory">
      <header className="instruction-section-head">
        <div>
          <h3>Claude 自动记忆</h3>
          <p className="muted">
            会话开始时会把各仓库 MEMORY.md 的开头注入上下文。这里只读，不能改也不能删。
          </p>
        </div>
      </header>
      <ul className="instruction-memory-list">
        {repos.map((repo) => {
          const open = openRepo === repo.abs_path;
          return (
            <li key={repo.abs_path} className="instruction-memory-item">
              <button
                type="button"
                className="instruction-memory-head"
                onClick={() =>
                  setOpenRepo((current) => (current === repo.abs_path ? null : repo.abs_path))
                }
              >
                <div className="instruction-memory-title">
                  <strong>{projectLabel(repo.repo)}</strong>
                  <span>{repo.repo}</span>
                </div>
                <div className="instruction-memory-meta">
                  <span>{formatBytes(repo.byte_size)}</span>
                  <span>{formatClock(repo.modified_at)}</span>
                </div>
              </button>
              {open ? (
                <div className="instruction-memory-body">
                  {repo.files.map((file) => (
                    <article key={file.abs_path} className="instruction-memory-file">
                      <header>
                        <strong>{file.name}</strong>
                        <span>{formatBytes(file.byte_size)}</span>
                        {file.name === "MEMORY.md" ? (
                          <em>会话开始时注入开头</em>
                        ) : (
                          <em>按需加载</em>
                        )}
                      </header>
                      <pre>{file.content}</pre>
                    </article>
                  ))}
                </div>
              ) : null}
            </li>
          );
        })}
      </ul>
    </section>
  );
}
