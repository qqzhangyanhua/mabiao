import { useLayoutEffect, useRef } from "react";
import { paintFuseBeadPoster } from "./fuseBeadPaint";
import type { ReportPosterRenderProps } from "./posterStyleRegistry";
import "./fuseBeadPoster.css";

/** `fuse-bead`：canvas 绘拼豆海报。只消费共享 PosterViewModel。 */
export function FuseBeadPoster({
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
        paintFuseBeadPoster(canvas, data);
      }
    };
    paint();
    void document.fonts.ready.then(paint);
    return () => {
      cancelled = true;
    };
  }, [data]);

  return (
    <article ref={posterRef} id={posterId} className="fb-poster">
      <canvas
        ref={canvasRef}
        data-poster-canvas=""
        role="img"
        aria-label={`${data.kicker} ${data.rangeLabel} ${data.totalTokensLabel}`}
      />
    </article>
  );
}
