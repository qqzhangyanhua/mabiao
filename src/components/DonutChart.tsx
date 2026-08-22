import type { EChartsOption } from "echarts";
import { ReactECharts } from "../lib/echarts";
import type { ChartEventHandler } from "./ExportableChart";

export function DonutChart({
  option,
  centerLabel = "总计",
  centerValue,
  onEvents,
}: {
  option: EChartsOption;
  centerLabel?: string;
  centerValue: string;
  onEvents?: Record<string, ChartEventHandler>;
}) {
  const valueSize =
    centerValue.length >= 10 ? "is-long" : centerValue.length >= 8 ? "is-medium" : "";

  return (
    <div className="donut-chart">
      <ReactECharts option={option} style={{ height: "100%", width: "100%" }} onEvents={onEvents} />
      <div className="donut-center">
        <span>{centerLabel}</span>
        <strong className={valueSize} title={centerValue}>
          {centerValue}
        </strong>
      </div>
    </div>
  );
}
