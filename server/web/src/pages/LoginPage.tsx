import { useState, type FormEvent } from "react";
import { useGuest } from "../auth";

export function LoginPage() {
  const { signIn } = useGuest();
  const [account, setAccount] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await signIn(account.trim(), password);
    } catch (e) {
      setError(e instanceof Error ? e.message : "登录失败");
      setBusy(false);
    }
  }

  return (
    <div className="flex min-h-screen items-center justify-center px-4">
      <form
        onSubmit={submit}
        className="w-full max-w-sm space-y-4 rounded border border-slate-200 bg-white p-6 shadow-sm"
      >
        <h1 className="text-lg font-semibold">码表 · 团队</h1>
        <p className="text-sm text-slate-500">
          用管理员给你开的账号登录。成员只能看到自己推送的数据。
        </p>
        <label className="block text-sm">
          账号
          <input
            value={account}
            onChange={(e) => setAccount(e.target.value)}
            autoComplete="username"
            required
            className="mt-1 w-full rounded border border-slate-300 px-3 py-2"
          />
        </label>
        <label className="block text-sm">
          密码
          <input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            autoComplete="current-password"
            required
            className="mt-1 w-full rounded border border-slate-300 px-3 py-2"
          />
        </label>
        {error && (
          <p role="alert" className="text-sm text-red-700">
            {error}
          </p>
        )}
        <button
          type="submit"
          disabled={busy}
          className="w-full rounded bg-blue-600 py-2 text-white hover:bg-blue-700 disabled:opacity-60"
        >
          {busy ? "登录中…" : "登录"}
        </button>
      </form>
    </div>
  );
}
