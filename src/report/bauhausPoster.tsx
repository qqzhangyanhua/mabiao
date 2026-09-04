import { useLayoutEffect, useRef } from "react";
import { paintBauhausPoster } from "./bauhausPaint";
import type { ReportPosterRenderProps } from "./posterStyleRegistry";
import "./bauhausPoster.css";

/** `bauhaus-print`：canvas 绘纸面构成。只消费共享 PosterViewModel。 */
export function BauhausPoster({
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
        paintBauhausPoster(canvas, data);
      }
    };
    paint();
    void document.fonts.ready.then(paint);
    return () => {
      cancelled = true;
    };
  }, [data]);

  return (
    <article ref={posterRef} id={posterId} className="bh-poster">
      <canvas
        ref={canvasRef}
        data-poster-canvas=""
        role="img"
        aria-label={`${data.kicker} ${data.rangeLabel} ${data.totalTokensLabel}`}
      />
    </article>
  );
}
