import { StrictMode, useCallback, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { capturePoster } from "./capturePoster";
import { EXTREME_POSTERS, FAKE_POSTER } from "./fakePosterData";
import { ReportPoster } from "./ReportPoster";
import {
  REPORT_POSTER_STYLES,
  resolveReportPosterStyleId,
  type ReportPosterStyleId,
} from "./posterStyleRegistry";
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

function queryParams(): URLSearchParams {
  return new URLSearchParams(window.location.search);
}

function selectedExtreme(): (typeof EXTREME_POSTERS)[number] | undefined {
  const id = queryParams().get("case");
  if (!id) {
    return undefined;
  }
  return EXTREME_POSTERS.find((item) => item.id === id);
}

function selectedStyleId(): ReportPosterStyleId {
  return resolveReportPosterStyleId(queryParams().get("style"));
}

function replaceQuery(next: { style?: ReportPosterStyleId }): void {
  const params = queryParams();
  params.set("style", next.style ?? selectedStyleId());
  const search = params.toString();
  window.history.replaceState(null, "", `${window.location.pathname}?${search}`);
}

function SpikeStyleSwitch({
  selected,
  onSelect,
}: {
  selected: ReportPosterStyleId;
  onSelect: (styleId: ReportPosterStyleId) => void;
}) {
  return (
    <div className="spike-styles" role="radiogroup" aria-label="周报风格">
      {REPORT_POSTER_STYLES.map((style) => {
        const active = style.id === selected;
        return (
          <button
            key={style.id}
            type="button"
            role="radio"
            aria-checked={active}
            className={active ? "spike-style is-active" : "spike-style"}
            onClick={() => onSelect(style.id)}
          >
            <span
              className="spike-style-swatch"
              aria-hidden="true"
              style={{
                background: style.swatch.background,
                boxShadow: `inset 0 0 0 2px ${style.swatch.accent}`,
              }}
            />
            {style.label}
          </button>
        );
      })}
    </div>
  );
}

function SpikeApp() {
  const extreme = selectedExtreme();
  const [styleId, setStyleId] = useState<ReportPosterStyleId>(() => selectedStyleId());
  const [png, setPng] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const runId = useRef(0);

  const chooseStyle = useCallback((next: ReportPosterStyleId) => {
    setStyleId(next);
    setPng(null);
    replaceQuery({ style: next });
  }, []);

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
        <p className="spike-focus-note">
          {extreme.label} · 下列为全部已注册风格。打包后的 Tauri WKWebView 里请按风格各复制一次。
        </p>
        <div className="spike-style-gallery">
          {REPORT_POSTER_STYLES.map((style) => (
            <div key={style.id} className="spike-col">
              <h3>
                {style.label} <code>{style.id}</code>
              </h3>
              <ReportPoster
                data={extreme.data}
                styleId={style.id}
                posterId={`report-poster-${extreme.id}-${style.id}`}
              />
            </div>
          ))}
        </div>
      </div>
    );
  }

  return (
    <div className="spike">
      <header className="spike-head">
        <h1>报告海报 spike</h1>
        <p>
          验证 modern-screenshot 的 foreignObject 路线。此页不进入产品导航，也不改分享对话框。新增周报风格后会自动出现在下方对照里。
        </p>
        <p>
          目视：浏览器里并排看各风格的屏上渲染；每种风格点一次截图，对照右侧 PNG。打包后的 Tauri
          WKWebView 里按风格各复制一次，粘贴核对玻璃模糊、霓虹发光、中文、透明圆角、按天柱与来源条。CI
          不做像素快照。
        </p>
        <SpikeStyleSwitch selected={styleId} onSelect={chooseStyle} />
        <div className="spike-status">
          <button type="button" data-spike="capture-btn" onClick={() => void runCapture()} disabled={busy}>
            {busy ? "截图中" : png ? "重新截图" : "截图"}
          </button>
          <span className={`spike-msg${error ? " is-error" : ""}`}>
            {error ??
              (png
                ? "已截出 PNG，左侧屏上 / 右侧 foreignObject"
                : "点截图，对比当前风格的屏上渲染和 foreignObject PNG")}
          </span>
        </div>
      </header>
      <div className="spike-grid">
        <section className="spike-col">
          <h2>屏上渲染 · {styleId}</h2>
          <ReportPoster data={FAKE_POSTER} styleId={styleId} />
        </section>
        <section className="spike-col">
          <h2>foreignObject PNG</h2>
          {png ? (
            <img className="spike-capture" data-spike="capture" alt="foreignObject 截图" src={png} />
          ) : null}
        </section>
      </div>
      <section className="spike-gallery">
        <h2>全部已注册风格</h2>
        <p>同一份假数据。切换风格只改视觉，不改七个槽位的数字。</p>
        <div className="spike-style-gallery">
          {REPORT_POSTER_STYLES.map((style) => (
            <div key={style.id} className="spike-col">
              <h3>
                {style.label} <code>{style.id}</code>
              </h3>
              <ReportPoster data={FAKE_POSTER} styleId={style.id} posterId={`report-poster-all-${style.id}`} />
            </div>
          ))}
        </div>
      </section>
      <section className="spike-extremes">
        <h2>极端数据整图</h2>
        <p>稀疏与数值极端的组合，当前风格 {styleId}。目视确认没有空槽位或占位符。</p>
        <div className="spike-extreme-list">
          {EXTREME_POSTERS.map((item) => (
            <div key={item.id} className="spike-col">
              <h3>{item.label}</h3>
              <ReportPoster
                data={item.data}
                styleId={styleId}
                posterId={`report-poster-${item.id}`}
              />
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
