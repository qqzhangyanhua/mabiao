/**
 * 后端把附件按 `data:` URL 传过来，直接塞进 `<img src>` 会让整张图以 base64 字符串常驻
 * JS 堆（比二进制还大 33%），解码后的位图又另算一份。转成 blob URL 后，字符串可以立刻
 * 回收，图片数据由 webview 自己管理，并且能在不需要时用 `revokeObjectURL` 明确释放。
 */

const DATA_URL_PATTERN = /^data:([^,]*),([\s\S]*)$/;

export function dataUrlToBlob(dataUrl: string): Blob | null {
  const match = DATA_URL_PATTERN.exec(dataUrl);
  if (!match) {
    return null;
  }
  const parameters = match[1].split(";");
  const mediaType = parameters[0] || "application/octet-stream";
  const isBase64 = parameters.slice(1).some((parameter) => parameter.toLowerCase() === "base64");
  const payload = match[2];

  if (!isBase64) {
    try {
      return new Blob([decodeURIComponent(payload)], { type: mediaType });
    } catch {
      return null;
    }
  }

  try {
    const binary = atob(payload);
    const bytes = new Uint8Array(binary.length);
    for (let index = 0; index < binary.length; index += 1) {
      bytes[index] = binary.charCodeAt(index);
    }
    return new Blob([bytes], { type: mediaType });
  } catch {
    return null;
  }
}
