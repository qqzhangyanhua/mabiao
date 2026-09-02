import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";
import { Button } from "./ui/Button";

type Busy = "idle" | "backup" | "restore";

export function BackupPanel({ onRestored }: { onRestored?: () => void }) {
  const [busy, setBusy] = useState<Busy>("idle");
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function handleBackup() {
    setBusy("backup");
    setMessage(null);
    setError(null);
    try {
      const saved = await invoke<boolean>("backup_data");
      setMessage(saved ? "已写入备份目录" : null);
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy("idle");
    }
  }

  async function handleRestore() {
    const confirmed = window.confirm(
      "恢复会覆盖当前用量缓存、单价表、月度预算、扫描路径、官方额度配置、自定义提供商配置和通知状态，且成功后无法自动撤回。自定义提供商密钥不会被覆盖。确定继续？",
    );
    if (!confirmed) {
      return;
    }
    setBusy("restore");
    setMessage(null);
    setError(null);
    try {
      const restored = await invoke<boolean>("restore_data");
      if (restored) {
        setMessage("已恢复备份，当前缓存与单价/预算已被覆盖");
        onRestored?.();
      }
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy("idle");
    }
  }

  return (
    <section className="panel" id="settings-backup">
      <div className="panel-head">
        <div>
          <h2>数据备份与恢复</h2>
          <p className="panel-note">
            备份本机用量缓存（sqlite）、单价表、月度预算、扫描路径、预算通知状态、LiteLLM
            价目快照和自定义提供商配置。密钥不进备份。恢复会覆盖当前缓存，但不会改写本机已有的自定义提供商密钥。
          </p>
        </div>
        <div className="row-actions">
          <Button variant="accent" disabled={busy !== "idle"} onClick={() => void handleBackup()}>
            {busy === "backup" ? "备份中…" : "备份"}
          </Button>
          <Button disabled={busy !== "idle"} onClick={() => void handleRestore()}>
            {busy === "restore" ? "恢复中…" : "恢复"}
          </Button>
        </div>
      </div>
      {message ? <p className="panel-note">{message}</p> : null}
      {error ? (
        <p className="panel-note snapshot-error" role="alert">
          {error}
        </p>
      ) : null}
    </section>
  );
}
