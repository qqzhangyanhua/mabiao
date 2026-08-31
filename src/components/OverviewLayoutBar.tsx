import { useState } from "react";
import { Icon } from "../icons";
import { sourceLabel } from "../lib/format";
import {
  defaultOverviewLayout,
  officialQuotaProviderLabel,
  OVERVIEW_MODULE_LABELS,
  summarizeOverviewLayout,
  type OverviewLayout,
} from "../lib/overviewLayout";
import { OverviewLayoutControls } from "./OverviewLayoutControls";
import { Button } from "./ui/Button";

export function OverviewLayoutBar({
  layout,
  detectedSources,
  presentSources,
  onChange,
}: {
  layout: OverviewLayout;
  detectedSources: string[];
  presentSources: string[];
  onChange: (layout: OverviewLayout) => void;
}) {
  const [open, setOpen] = useState(false);
  const summary = formatLayoutSummary(layout, presentSources);

  return (
    <section className="overview-layout-bar">
      <div className="overview-layout-bar-head">
        <Button
          variant="text"
          className={open ? "overview-layout-trigger is-open" : "overview-layout-trigger"}
          aria-expanded={open}
          aria-controls="overview-layout-editor"
          onClick={() => setOpen((prev) => !prev)}
        >
          {open ? "收起显示配置" : "配置显示"}
          <Icon name="chevron" size={12} className="caret" />
        </Button>
        <span className="muted">{summary}</span>
        {open ? (
          <div className="overview-layout-bar-actions">
            <Button
              variant="text"
              className="overview-layout-action"
              onClick={() => onChange(defaultOverviewLayout())}
            >
              恢复默认
            </Button>
          </div>
        ) : null}
      </div>
      {open ? (
        <div id="overview-layout-editor" className="overview-layout-editor">
          <OverviewLayoutControls
            layout={layout}
            detectedSources={detectedSources}
            presentSources={presentSources}
            onChange={onChange}
          />
        </div>
      ) : null}
    </section>
  );
}

function formatLayoutSummary(layout: OverviewLayout, presentSources: string[]): string {
  const { hiddenModules, hiddenPresentSources, hiddenOfficialProviders } = summarizeOverviewLayout(
    layout,
    presentSources,
  );
  if (
    hiddenModules.length === 0 &&
    hiddenPresentSources.length === 0 &&
    hiddenOfficialProviders.length === 0
  ) {
    return "全部模块与额度来源均显示";
  }
  const parts: string[] = [];
  if (hiddenModules.length > 0) {
    parts.push(`已隐藏 ${hiddenModules.map((id) => OVERVIEW_MODULE_LABELS[id]).join("、")}`);
  }
  if (hiddenOfficialProviders.length > 0) {
    parts.push(
      `官方额度未显示 ${hiddenOfficialProviders.map((id) => officialQuotaProviderLabel(id)).join("、")}`,
    );
  }
  if (hiddenPresentSources.length > 0) {
    parts.push(`额度未显示 ${hiddenPresentSources.map((id) => sourceLabel(id)).join("、")}`);
  }
  return parts.join(" · ");
}
