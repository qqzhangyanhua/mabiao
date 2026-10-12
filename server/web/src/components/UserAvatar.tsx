/** 同一会话固定一张头像，换会话才变色。 */
function hueOf(seed: string): number {
  let hash = 2166136261;
  for (let index = 0; index < seed.length; index += 1) {
    hash ^= seed.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return (hash >>> 0) % 360;
}

export function UserAvatar({ seed, size = 32 }: { seed: string; size?: number }) {
  const hue = hueOf(seed || "user");
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 32 32"
      aria-hidden
      style={{ display: "block", overflow: "hidden", borderRadius: "999px" }}
    >
      <rect width="32" height="32" rx="16" fill={`hsl(${hue} 42% 46%)`} />
      <circle cx="16" cy="12.5" r="5" fill="rgba(255,255,255,0.94)" />
      <path d="M6.5 27.5c1.4-5.2 4.6-7.6 9.5-7.6s8.1 2.4 9.5 7.6" fill="rgba(255,255,255,0.94)" />
    </svg>
  );
}
