import { domToPng } from "modern-screenshot";

function waitTwoFrames(): Promise<void> {
  return new Promise((resolve) => {
    requestAnimationFrame(() => {
      requestAnimationFrame(() => resolve());
    });
  });
}

/**
 * 用 modern-screenshot 的 foreignObject 路线把海报节点截成 PNG data URL。
 * 调用方负责传入海报根节点；空结果视为失败，不静默返回。
 */
export async function capturePoster(node: HTMLElement): Promise<string> {
  await document.fonts.ready;
  await waitTwoFrames();
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
