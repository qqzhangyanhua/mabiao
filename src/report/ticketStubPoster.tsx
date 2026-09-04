import { useLayoutEffect, useRef } from "react";
import { paintTicketStubPoster } from "./ticketStubPaint";
import type { ReportPosterRenderProps } from "./posterStyleRegistry";
import "./ticketStubPoster.css";

/** `ticket-stub`：canvas 绘票据存根。只消费共享 PosterViewModel。 */
export function TicketStubPoster({
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
        paintTicketStubPoster(canvas, data);
      }
    };
    paint();
    void document.fonts.ready.then(paint);
    return () => {
      cancelled = true;
    };
  }, [data]);

  return (
    <article ref={posterRef} id={posterId} className="ts-poster">
      <canvas
        ref={canvasRef}
        data-poster-canvas=""
        role="img"
        aria-label={`${data.kicker} ${data.rangeLabel} ${data.totalTokensLabel}`}
      />
    </article>
  );
}
