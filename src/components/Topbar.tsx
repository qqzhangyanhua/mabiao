import { useState, type ReactNode } from "react";
import { Icon, type IconName } from "../icons";
import { useAnchoredPanel } from "../hooks/useAnchoredPanel";
import { useDismissible } from "../hooks/useDismissible";
import {
  conversationApplicationLabel,
  conversationSourceOptions,
} from "../lib/conversationDisplay";
import {
  applicationLabel,
  applicationSourceOptions,
  customRangeFilter,
  formatRangeLabel,
  projectLabel,
  providerChannel,
} from "../lib/format";
import {
  clearDimensionFilters,
  filterChips,
  removeFilterChip,
  type FilterChip,
} from "../lib/filterChips";
import type { Filter, FilterOptions, View } from "../types";
import { viewTitle } from "./Sidebar";
import { RangeBackButton } from "./RangeBackButton";
import { Button } from "./ui/Button";
import { DatePicker } from "./ui/DatePicker";
import { Select } from "./ui/Select";
import { SourceIcon } from "./SourceIcon";
import { VendorIcon } from "./VendorIcon";

const RANGE_OPTIONS = [
  { value: "today", label: "今天" },
  { value: "7", label: "近 7 天" },
  { value: "30", label: "近 30 天" },
  { value: "month", label: "本月" },
  { value: "all", label: "全部历史" },
  { value: "custom", label: "自定义区间" },
];

function chipLabel(chip: FilterChip): string {
  if (chip.kind === "project") {
    return projectLabel(chip.value);
  }
  if (chip.kind === "source") {
    return applicationLabel(chip.value);
  }
  if (chip.kind === "provider") {
    const name = chip.value || "（未标注）";
    return `${name}（${providerChannel(chip.value)}）`;
  }
  return chip.value;
}

export function Topbar({
  view,
  filter,
  preset,
  options,
  disabled,
  refreshDisabled = false,
  onPreset,
  onChange,
  onRangeBack,
  onRefresh,
}: {
  view: View;
  filter: Filter;
  preset: string;
  options: FilterOptions;
  disabled: boolean;
  refreshDisabled?: boolean;
  onPreset: (preset: string, range?: { from: string | null; to: string | null }) => void;
  onChange: (filter: Filter) => void;
  onRangeBack?: () => void;
  onRefresh: () => void;
}) {
  const { title, subtitle } = viewTitle(view);
  const hideAllFilters =
    view === "cursor" ||
    view === "cursor-sessions" ||
    view === "worktime" ||
    view === "instructions" ||
    view === "settings";
  const showSharedDimensionFilters = !hideAllFilters;
  const showUsageOnlyFilters = showSharedDimensionFilters && view !== "conversations";
  const sourceOptions =
    view === "conversations"
      ? conversationSourceOptions(options.sources)
      : view === "application" || view === "trend" || view === "project"
        ? applicationSourceOptions(options.sources)
        : options.sources;
  const sourceLabel = view === "conversations" ? conversationApplicationLabel : applicationLabel;
  const committedFrom = (filter.from ?? "").slice(0, 10);
  const committedTo = (filter.to ?? "").slice(0, 10);
  const rangeKey = `${preset}:${filter.from ?? ""}:${filter.to ?? ""}`;
  const [draft, setDraft] = useState({
    key: rangeKey,
    from: committedFrom,
    to: committedTo,
  });
  if (draft.key !== rangeKey) {
    setDraft({ key: rangeKey, from: committedFrom, to: committedTo });
  }
  const customOpen = preset === "custom";
  const customFrom = draft.key === rangeKey ? draft.from : committedFrom;
  const customTo = draft.key === rangeKey ? draft.to : committedTo;
  const chips = filterChips(filter).filter(
    (chip) => showUsageOnlyFilters || chip.kind === "project" || chip.kind === "source",
  );

  function selectPreset(value: string) {
    if (value === "custom") {
      if (preset !== "custom") {
        onPreset("custom", { from: filter.from, to: filter.to });
      }
      return;
    }
    onPreset(value);
  }

  function applyCustomRange() {
    if (!customFrom || !customTo) {
      return;
    }
    onPreset("custom", customRangeFilter(customFrom, customTo));
  }

  return (
    <header className="topbar">
      <div className="topbar-main">
        <div>
          <h1>{title}</h1>
          <p>{subtitle}</p>
        </div>
        {showSharedDimensionFilters ? (
          <div className="topbar-actions">
            {showUsageOnlyFilters && onRangeBack ? (
              <RangeBackButton disabled={disabled} onClick={onRangeBack} />
            ) : null}
            {showUsageOnlyFilters ? (
              <Select
                icon="calendar"
                ariaLabel="时间范围"
                disabled={disabled}
                value={customOpen ? "custom" : preset}
                displayLabel={customOpen ? "自定义区间" : formatRangeLabel(filter, preset)}
                options={RANGE_OPTIONS}
                onChange={selectPreset}
              />
            ) : null}
            {showUsageOnlyFilters && customOpen ? (
              <div className="custom-range">
                <DatePicker
                  ariaLabel="开始日期"
                  disabled={disabled}
                  value={customFrom}
                  max={customTo || undefined}
                  onChange={(from) => setDraft({ key: rangeKey, from, to: customTo })}
                />
                <span>至</span>
                <DatePicker
                  ariaLabel="结束日期"
                  disabled={disabled}
                  value={customTo}
                  min={customFrom || undefined}
                  onChange={(to) => setDraft({ key: rangeKey, from: customFrom, to })}
                />
                <Button
                  variant="text"
                  disabled={disabled || !customFrom || !customTo}
                  onClick={applyCustomRange}
                >
                  应用
                </Button>
              </div>
            ) : null}
            <Button
              variant="icon"
              disabled={disabled || refreshDisabled}
              onClick={onRefresh}
              title="刷新（R）"
              aria-label="刷新数据"
            >
              <Icon name="refresh" size={15} />
            </Button>
            <MultiSelect
              label="全部项目"
              options={options.projects}
              selected={filter.projects}
              renderLabel={projectLabel}
              disabled={disabled}
              onChange={(projects) => onChange({ ...filter, projects })}
            />
            <MultiSelect
              label="全部应用"
              icon="filter"
              options={sourceOptions}
              selected={filter.sources}
              renderLabel={sourceLabel}
              renderIcon={(source) => <SourceIcon source={source} size={14} />}
              disabled={disabled}
              onChange={(sources) => onChange({ ...filter, sources })}
            />
            {showUsageOnlyFilters ? (
              <MultiSelect
                label="全部模型"
                options={options.models}
                selected={filter.models}
                disabled={disabled}
                renderIcon={(model) => <VendorIcon name={model} size={14} />}
                onChange={(models) => onChange({ ...filter, models })}
              />
            ) : null}
            {showUsageOnlyFilters ? (
              <MultiSelect
                label="全部 Provider"
                options={options.providers}
                selected={filter.providers}
                disabled={disabled}
                renderLabel={(name) => `${name}（${providerChannel(name)}）`}
                onChange={(providers) => onChange({ ...filter, providers })}
              />
            ) : null}
          </div>
        ) : null}
      </div>
      {showSharedDimensionFilters && chips.length > 0 ? (
        <div className="filter-chips" aria-label="已选筛选">
          {chips.map((chip) => (
            <button
              key={chip.id}
              type="button"
              className="filter-chip"
              disabled={disabled}
              title={`移除 ${chipLabel(chip)}`}
              onClick={() => onChange(removeFilterChip(filter, chip))}
            >
              {chip.kind === "source" ? <SourceIcon source={chip.value} size={14} /> : null}
              <span>{chipLabel(chip)}</span>
              <Icon name="close" size={11} />
            </button>
          ))}
          <Button
            variant="text"
            disabled={disabled}
            onClick={() => onChange(clearDimensionFilters(filter))}
          >
            清空筛选
          </Button>
        </div>
      ) : null}
    </header>
  );
}

function MultiSelect({
  label,
  icon,
  options,
  selected,
  renderLabel,
  renderIcon,
  disabled,
  onChange,
}: {
  label: string;
  icon?: IconName;
  options: string[];
  selected: string[];
  renderLabel?: (value: string) => string;
  renderIcon?: (value: string) => ReactNode;
  disabled?: boolean;
  onChange: (values: string[]) => void;
}) {
  const { open, setOpen, rootRef } = useDismissible();
  const panelStyle = useAnchoredPanel(open, rootRef);
  const [query, setQuery] = useState("");
  const q = query.trim().toLowerCase();
  const visible = options.filter((option) => {
    const text = (renderLabel ? renderLabel(option) : option).toLowerCase();
    return q === "" || text.includes(q) || option.toLowerCase().includes(q);
  });

  function toggleValue(value: string) {
    if (selected.includes(value)) {
      onChange(selected.filter((item) => item !== value));
    } else {
      onChange([...selected, value]);
    }
  }

  const summary =
    selected.length === 0
      ? label
      : selected.length === 1
        ? renderLabel
          ? renderLabel(selected[0] ?? "")
          : (selected[0] ?? label)
        : `已选 ${selected.length} 项`;

  return (
    <div className="multi-select" ref={rootRef}>
      <button
        type="button"
        className="chip-field multi-select-trigger"
        disabled={disabled}
        onClick={() => setOpen((value) => !value)}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={`${label}：${summary}`}
      >
        {selected.length === 1 && selected[0] && renderIcon ? renderIcon(selected[0]) : null}
        {icon ? <Icon name={icon} size={14} /> : null}
        <span className="chip-range">{summary}</span>
        <Icon name="chevron" size={12} className={open ? "select-caret open" : "select-caret"} />
      </button>
      {open ? (
        <div className="multi-select-panel" role="listbox" aria-label={label} style={panelStyle}>
          <input
            className="multi-select-search"
            type="search"
            placeholder="搜索…"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            aria-label={`${label}搜索`}
          />
          <div className="multi-select-actions">
            <Button variant="text" onClick={() => onChange([])}>
              清空
            </Button>
            <Button variant="text" onClick={() => onChange(options)}>
              全选
            </Button>
          </div>
          <div className="multi-select-list">
            {visible.map((option) => (
              <label className="multi-select-item" key={option}>
                <input
                  type="checkbox"
                  checked={selected.includes(option)}
                  onChange={() => toggleValue(option)}
                />
                {renderIcon ? renderIcon(option) : null}
                <span>{renderLabel ? renderLabel(option) : option}</span>
              </label>
            ))}
            {visible.length === 0 ? <div className="multi-select-empty">无匹配项</div> : null}
          </div>
        </div>
      ) : null}
    </div>
  );
}
