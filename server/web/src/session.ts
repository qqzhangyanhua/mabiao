import type { Role } from "./api/types";

export interface Profile {
  id: number;
  account: string;
  role: Role;
}

// sessionStorage：关掉标签页就退出，token 不长期留在浏览器里。
const TOKEN_KEY = "mabiao.token";
const PROFILE_KEY = "mabiao.profile";

export function readToken(): string | null {
  return sessionStorage.getItem(TOKEN_KEY);
}

export function saveToken(token: string): void {
  sessionStorage.setItem(TOKEN_KEY, token);
}

export function saveProfile(profile: Profile): void {
  sessionStorage.setItem(PROFILE_KEY, JSON.stringify(profile));
}

export function loadProfile(): Profile | null {
  if (!readToken()) return null;
  const raw = sessionStorage.getItem(PROFILE_KEY);
  if (!raw) return null;
  try {
    const value = JSON.parse(raw) as Partial<Profile>;
    if (
      typeof value.id === "number" &&
      typeof value.account === "string" &&
      (value.role === "admin" || value.role === "member")
    ) {
      return { id: value.id, account: value.account, role: value.role };
    }
  } catch {
    // 被改坏了就当没登录。
  }
  return null;
}

export function clearSession(): void {
  sessionStorage.removeItem(TOKEN_KEY);
  sessionStorage.removeItem(PROFILE_KEY);
}
