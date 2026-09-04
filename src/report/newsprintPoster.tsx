import { useLayoutEffect, useRef } from "react";
import { paintNewsprintPoster } from "./newsprintPaint";
import type { ReportPosterRenderProps } from "./posterStyleRegistry";
import "./newsprintPoster.css";

/** `newsprint`：canvas 绘旧报号外。只消费共享 PosterViewModel。 */
export function NewsprintPoster({
  data,
  posterRef,
  posterId = "report-poster",
}: ReportPosterRenderProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useLayoutEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) {
      return;
    }
    let cancelled = false;
    const paint = () => {
      if (!cancelled) {
        paintNewsprintPoster(canvas, data);
      }
    };
    paint();
    void document.fonts.ready.then(paint);
    return () => {
      cancelled = true;
    };
  }, [data]);

  return (
    <article ref={posterRef} id={posterId} className="np-poster">
      <canvas
        ref={canvasRef}
        data-poster-canvas=""
        role="img"
        aria-label={`${data.kicker} ${data.rangeLabel} ${data.totalTokensLabel}`}
      />
    </article>
  );
}
