import {
  resolveReportPosterStyle,
  type ReportPosterRenderProps,
} from "./posterStyleRegistry";

export function ReportPoster({
  styleId,
  ...renderProps
}: ReportPosterRenderProps & { styleId?: string | null }) {
  const Style = resolveReportPosterStyle(styleId).Component;
  return <Style {...renderProps} />;
}
