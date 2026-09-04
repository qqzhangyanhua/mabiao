import { domToPng } from "modern-screenshot";

function waitTwoFrames(): Promise<void> {
  return new Promise((resolve) => {
    requestAnimationFrame(() => {
      requestAnimationFrame(() => resolve());
    });
  });
}

function isPosterCanvas(node: Element | null): node is HTMLCanvasElement {
  return (
    node != null && typeof HTMLCanvasElement !== "undefined" && node instanceof HTMLCanvasElement
  );
}

/**
 * 把海报节点截成 PNG data URL。
 * canvas 海报（构成 / 旧报 / 水墨 / 票据 / 拼豆 / 混凝土）直接导出位图，foreignObject 拿不到画布像素；其余风格仍用 foreignObject。
 * 调用方负责传入海报根节点；空结果视为失败，不静默返回。
 */
export async function capturePoster(node: HTMLElement): Promise<string> {
  await document.fonts.ready;
  await waitTwoFrames();
  const canvas = node.querySelector("[data-poster-canvas]");
  if (isPosterCanvas(canvas)) {
    const dataUrl = canvas.toDataURL("image/png");
    if (!dataUrl || dataUrl === "data:,") {
      throw new Error("截图结果为空");
    }
    return dataUrl;
  }
  const dataUrl = await domToPng(node, {
    scale: 2,
    backgroundColor: null,
    features: {
      copyScrollbar: false,
      fixSvgXmlDecode: true,
    },
  });
  if (!dataUrl) {
    throw new Error("截图结果为空");
  }
  return dataUrl;
}
