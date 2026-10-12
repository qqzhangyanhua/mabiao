import { useState, type FormEvent } from "react";
import { useGuest } from "../auth";
import { Button } from "../components/ui/button";
import { glassClass } from "../components/ui/card";
import { FieldLabel, Input } from "../components/ui/input";
import { cn } from "../lib/cn";

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
      <div className={cn(glassClass, "w-full max-w-sm p-6")}>
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
          <label className="flex flex-col gap-1 text-sm">
            <FieldLabel>账号</FieldLabel>
            <Input
              value={account}
              onChange={(e) => setAccount(e.target.value)}
              autoComplete="username"
              required
            />
          </label>
          <label className="flex flex-col gap-1 text-sm">
            <FieldLabel>密码</FieldLabel>
            <Input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              autoComplete="current-password"
              required
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
      </div>
    </div>
  );
}
