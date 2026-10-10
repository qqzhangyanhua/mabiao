import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react";
import { createApi, type Api } from "./api/client";
import {
  clearSession,
  loadProfile,
  readToken,
  saveProfile,
  saveToken,
  type Profile,
} from "./session";

interface AuthValue {
  api: Api;
  profile: Profile;
  logout: () => void;
}

interface GuestValue {
  api: Api;
  signIn: (account: string, password: string) => Promise<void>;
}

const AuthContext = createContext<AuthValue | null>(null);
const GuestContext = createContext<GuestValue | null>(null);

export function useAuth(): AuthValue {
  const value = useContext(AuthContext);
  if (!value) throw new Error("useAuth 只能在登录后的页面里用");
  return value;
}

export function useGuest(): GuestValue {
  const value = useContext(GuestContext);
  if (!value) throw new Error("useGuest 只能在登录页里用");
  return value;
}

/** 登录前给登录页，登录后给其余页面；token 失效（401）时自动退回登录页。 */
export function AuthProvider({
  signedIn,
  signedOut,
}: {
  signedIn: ReactNode;
  signedOut: ReactNode;
}) {
  const [profile, setProfile] = useState<Profile | null>(loadProfile);

  const api = useMemo(
    () =>
      createApi({
        getToken: readToken,
        onUnauthorized: () => {
          clearSession();
          setProfile(null);
        },
      }),
    [],
  );

  const logout = useCallback(() => {
    clearSession();
    setProfile(null);
  }, []);

  const signIn = useCallback(
    async (account: string, password: string) => {
      const login = await api.login(account, password);
      saveToken(login.token);
      try {
        const me = await api.me();
        const next: Profile = { id: me.id, account: me.account, role: me.role };
        saveProfile(next);
        setProfile(next);
      } catch (error) {
        clearSession();
        throw error;
      }
    },
    [api],
  );

  if (!profile) {
    return <GuestContext.Provider value={{ api, signIn }}>{signedOut}</GuestContext.Provider>;
  }
  return <AuthContext.Provider value={{ api, profile, logout }}>{signedIn}</AuthContext.Provider>;
}
