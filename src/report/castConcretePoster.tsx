import { useLayoutEffect, useRef } from "react";
import { paintCastConcretePoster } from "./castConcretePaint";
import type { ReportPosterRenderProps } from "./posterStyleRegistry";
import "./castConcretePoster.css";

/** `cast-concrete`：canvas 绘清水混凝土。只消费共享 PosterViewModel。 */
export function CastConcretePoster({
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
        paintCastConcretePoster(canvas, data);
      }
    };
    paint();
    void document.fonts.ready.then(paint);
    return () => {
      cancelled = true;
    };
  }, [data]);

  return (
    <article ref={posterRef} id={posterId} className="cc-poster">
      <canvas
        ref={canvasRef}
        data-poster-canvas=""
        role="img"
        aria-label={`${data.kicker} ${data.rangeLabel} ${data.totalTokensLabel}`}
      />
    </article>
  );
}
