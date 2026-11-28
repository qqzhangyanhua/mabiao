import { defaultOverviewLayout, type OverviewLayout } from "../lib/overviewLayout";
import { OverviewLayoutControls } from "./OverviewLayoutControls";
import { Button } from "./ui/Button";

export function OverviewLayoutPanel({
  layout,
  hiddenOfficialProviders,
  detectedSources,
  presentSources,
  onChange,
  onHiddenOfficialProvidersChange,
}: {
  layout: OverviewLayout;
  hiddenOfficialProviders: readonly string[];
  detectedSources: string[];
  presentSources?: string[];
  onChange: (layout: OverviewLayout) => void;
  onHiddenOfficialProvidersChange: (hiddenProviders: string[]) => void;
}) {
  function resetAll() {
    onChange(defaultOverviewLayout());
    if (hiddenOfficialProviders.length > 0) {
      onHiddenOfficialProvidersChange([]);
    }
  }

  return (
    <section className="panel" id="settings-overview">
      <div className="panel-head">
        <div>
          <h2>概览显示</h2>
          <p className="panel-note">
            配置首页展示哪些模块、官方额度账号，以及计费窗里显示哪些来源。偏好保存在本机，不影响统计缓存。
          </p>
        </div>
        <Button onClick={resetAll}>恢复默认</Button>
      </div>
      <OverviewLayoutControls
        layout={layout}
        hiddenOfficialProviders={hiddenOfficialProviders}
        detectedSources={detectedSources}
        presentSources={presentSources}
        onChange={onChange}
        onHiddenOfficialProvidersChange={onHiddenOfficialProvidersChange}
      />
    </section>
  );
}
