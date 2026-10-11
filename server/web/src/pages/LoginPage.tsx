import { useState, type FormEvent } from "react";
import { useGuest } from "../auth";
import { Button } from "../components/ui/button";
import { Card } from "../components/ui/card";
import { Input } from "../components/ui/input";

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
      <Card className="w-full max-w-sm p-6">
        <form onSubmit={submit} className="space-y-4">
          <div className="flex items-center gap-3">
            <span className="grid size-9 place-items-center rounded-lg bg-primary text-sm font-semibold text-primary-foreground">
              码
            </span>
            <div>
              <h1 className="text-xl font-normal tracking-tight">码表 · 团队</h1>
              <p className="text-sm text-muted-foreground">远程服务管理网页</p>
            </div>
          </div>
          <p className="text-sm text-muted-foreground">
            用管理员给你开的账号登录。成员只能看到自己推送的数据；管理员可查看你推送的全部正文。
          </p>
          <label className="block text-sm">
            账号
            <Input
              value={account}
              onChange={(e) => setAccount(e.target.value)}
              autoComplete="username"
              required
              className="mt-1"
            />
          </label>
          <label className="block text-sm">
            密码
            <Input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              autoComplete="current-password"
              required
              className="mt-1"
            />
          </label>
          {error && (
            <p role="alert" className="text-sm text-destructive">
              {error}
            </p>
          )}
          <Button type="submit" disabled={busy} className="w-full">
            {busy ? "登录中…" : "登录"}
          </Button>
        </form>
      </Card>
    </div>
  );
}
