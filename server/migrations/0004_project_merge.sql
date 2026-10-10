-- 管理员手动合并项目（ADR 0026「项目归并」的人工覆盖）。
-- 被合并的项目行保留，用 merged_into 指向合并目标：它的归并键还在，之后推送带着同一个 remote
-- 或同名目录进来时，仍然归到目标项目，不会又长出一个重复项目。
-- merged_into 始终直指最终目标（不成链）：合并时把原先指向被合并项目的别名一并改指新目标。
ALTER TABLE projects
    ADD COLUMN merged_into BIGINT REFERENCES projects (id),
    ADD CONSTRAINT projects_not_merged_into_self CHECK (merged_into IS DISTINCT FROM id);
CREATE INDEX projects_merged_into_idx ON projects (merged_into) WHERE merged_into IS NOT NULL;
