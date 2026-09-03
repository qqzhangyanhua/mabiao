import { StrictMode, useCallback, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { capturePoster } from "./capturePoster";
import { EXTREME_POSTERS, FAKE_POSTER } from "./fakePosterData";
import { ReportPoster } from "./ReportPoster";
import "./spike.css";

function setSpikeFlag(
  name: "__SPIKE_PNG__" | "__SPIKE_READY__" | "__SPIKE_ERROR__",
  value: string | boolean | undefined,
): void {
  Reflect.set(window, name, value);
}

function posterNode(): HTMLElement {
  const node = document.getElementById("report-poster");
  if (!(node instanceof HTMLElement)) {
    throw new Error("找不到海报节点");
  }
  return node;
}

function selectedExtreme(): (typeof EXTREME_POSTERS)[number] | undefined {
  const id = new URLSearchParams(window.location.search).get("case");
  if (!id) {
    return undefined;
  }
  return EXTREME_POSTERS.find((item) => item.id === id);
}

function SpikeApp() {
  const extreme = selectedExtreme();
  const [png, setPng] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const runId = useRef(0);

  const runCapture = useCallback(async () => {
    const id = ++runId.current;
    setBusy(true);
    setError(null);
    setSpikeFlag("__SPIKE_READY__", false);
    setSpikeFlag("__SPIKE_ERROR__", undefined);
    try {
      const dataUrl = await capturePoster(posterNode());
      if (id !== runId.current) {
        return;
      }
      setPng(dataUrl);
      setSpikeFlag("__SPIKE_PNG__", dataUrl);
    } catch (caught) {
      if (id !== runId.current) {
        return;
      }
      const message = caught instanceof Error ? caught.message : String(caught);
      setError(message);
      setSpikeFlag("__SPIKE_ERROR__", message);
    } finally {
      if (id === runId.current) {
        setBusy(false);
        setSpikeFlag("__SPIKE_READY__", true);
      }
    }
  }, []);

  if (extreme) {
    return (
      <div className="spike spike-focus">
        <ReportPoster data={extreme.data} />
      </div>
    );
  }

  return (
    <div className="spike">
      <header className="spike-head">
        <h1>报告海报 spike</h1>
        <p>验证 modern-screenshot 的 foreignObject 路线。此页不进入产品导航。</p>
        <div className="spike-status">
          <button
            type="button"
            data-spike="capture-btn"
            onClick={() => void runCapture()}
            disabled={busy}
          >
            {busy ? "截图中" : png ? "重新截图" : "截图"}
          </button>
          <span className={`spike-msg${error ? " is-error" : ""}`}>
            {error ??
              (png
                ? "已截出 PNG，左侧屏上 / 右侧 foreignObject"
                : "点截图，对比屏上渲染和 foreignObject PNG")}
          </span>
        </div>
      </header>
      <div className="spike-grid">
        <section className="spike-col">
          <h2>屏上渲染</h2>
          <ReportPoster data={FAKE_POSTER} />
        </section>
        <section className="spike-col">
          <h2>foreignObject PNG</h2>
          {png ? (
            <img
              className="spike-capture"
              data-spike="capture"
              alt="foreignObject 截图"
              src={png}
            />
          ) : null}
        </section>
      </div>
      <section className="spike-extremes">
        <h2>极端数据整图</h2>
        <p>稀疏与数值极端的组合。目视确认没有空槽位或占位符。</p>
        <div className="spike-extreme-list">
          {EXTREME_POSTERS.map((item) => (
            <div key={item.id} className="spike-col">
              <h3>{item.label}</h3>
              <ReportPoster data={item.data} posterId={`report-poster-${item.id}`} />
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}

const root = document.getElementById("root");
if (!root) {
  throw new Error("root element is missing");
}

createRoot(root).render(
  <StrictMode>
    <SpikeApp />
  </StrictMode>,
);
