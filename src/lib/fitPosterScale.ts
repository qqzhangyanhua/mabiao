/** 预览适配：把海报完整放进可视区，不放大。无效尺寸时保持 1，避免 scale(0)。 */
export function fitPosterScale(
  availableWidth: number,
  availableHeight: number,
  posterWidth: number,
  posterHeight: number,
): number {
  if (availableWidth <= 0 || availableHeight <= 0 || posterWidth <= 0 || posterHeight <= 0) {
    return 1;
  }
  return Math.min(availableWidth / posterWidth, availableHeight / posterHeight, 1);
}
