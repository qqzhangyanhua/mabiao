/**
 * 按需注册 echarts。
 *
 * `echarts-for-react` 的默认入口会把整个 echarts 拉进来（打包后 1 MB 出头），
 * 而本项目只用到折线、柱状、饼三种图和 grid/tooltip/legend 三个组件。
 * 解析后的 JS 代码是常驻内存的，所以这里改成核心 + 显式注册。
 *
 * 新增图表类型时，除了写 option，还必须在下面的 `use` 列表里注册对应模块，
 * 否则运行时图表会静默画不出来。
 */
import { BarChart, LineChart, PieChart } from "echarts/charts";
import { GridComponent, LegendComponent, TooltipComponent } from "echarts/components";
import * as echarts from "echarts/core";
import { CanvasRenderer } from "echarts/renderers";
import ReactEChartsCore from "echarts-for-react/lib/core";
import { createElement, forwardRef, type ComponentProps, type Ref } from "react";

echarts.use([
  LineChart,
  BarChart,
  PieChart,
  GridComponent,
  TooltipComponent,
  LegendComponent,
  CanvasRenderer,
]);

type CoreProps = Omit<ComponentProps<typeof ReactEChartsCore>, "echarts">;

export const ReactECharts = forwardRef(function ReactECharts(
  props: CoreProps,
  ref: Ref<ReactEChartsCore>,
) {
  return createElement(ReactEChartsCore, { ...props, echarts, ref });
});

export type ReactEChartsInstance = ReactEChartsCore;
