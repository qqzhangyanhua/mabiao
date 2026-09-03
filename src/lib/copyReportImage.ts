import { invoke } from "@tauri-apps/api/core";

/** 把海报 PNG data URL 交给后端写入系统剪贴板。不落盘。 */
export async function copyReportImage(dataUrl: string): Promise<void> {
  const comma = dataUrl.indexOf(",");
  if (comma < 0) {
    throw new Error("截图结果不是 PNG data URL");
  }
  const base64 = dataUrl.slice(comma + 1);
  if (!base64) {
    throw new Error("截图结果为空");
  }
  await invoke("copy_image_to_clipboard", { base64 });
}
