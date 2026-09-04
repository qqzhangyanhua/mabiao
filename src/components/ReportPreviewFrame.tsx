import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { fitPosterScale } from "../lib/fitPosterScale";

type Fit = {
  scale: number;
  width: number;
  height: number;
};

/**
 * 预览缩放包在海报节点外。缩放不得加在海报根节点上，否则截图会吃到 getBoundingClientRect 的视觉尺寸。
 */
export function ReportPreviewFrame({ children }: { children: ReactNode }) {
  const frameRef = useRef<HTMLDivElement>(null);
  const scalerRef = useRef<HTMLDivElement>(null);
  const [fit, setFit] = useState<Fit | null>(null);

  useLayoutEffect(() => {
    const frame = frameRef.current;
    const scaler = scalerRef.current;
    if (!frame || !scaler) {
      return;
    }

    const posterOf = () => {
      const child = scaler.firstElementChild;
      return child instanceof HTMLElement ? child : null;
    };

    let poster: HTMLElement | null = null;
    const measure = (ro: ResizeObserver) => {
      const next = posterOf();
      if (next !== poster) {
        if (poster) {
          ro.unobserve(poster);
        }
        poster = next;
        if (poster) {
          ro.observe(poster);
        }
      }
      const width = poster?.offsetWidth ?? 0;
      const height = poster?.offsetHeight ?? 0;
      const availableWidth = frame.clientWidth;
      const availableHeight = frame.clientHeight;
      if (width <= 0 || height <= 0 || availableWidth <= 0 || availableHeight <= 0) {
        return;
      }
      const scale = fitPosterScale(availableWidth, availableHeight, width, height);
      setFit((prev) => {
        if (prev && prev.scale === scale && prev.width === width && prev.height === height) {
          return prev;
        }
        return { scale, width, height };
      });
    };
    const ro = new ResizeObserver(() => {
      measure(ro);
    });

    ro.observe(frame);
    const mo = new MutationObserver(() => {
      measure(ro);
    });
    mo.observe(scaler, { childList: true });
    measure(ro);

    return () => {
      ro.disconnect();
      mo.disconnect();
    };
  }, []);

  const ready = fit != null;

  return (
    <div ref={frameRef} className="report-preview-frame">
      <div
        className={ready ? "report-preview-stage" : "report-preview-stage is-pending"}
        style={ready ? { width: fit.width * fit.scale, height: fit.height * fit.scale } : undefined}
      >
        <div
          ref={scalerRef}
          className="report-preview-scaler"
          style={ready ? { transform: `scale(${fit.scale})` } : undefined}
        >
          {children}
        </div>
      </div>
    </div>
  );
}
