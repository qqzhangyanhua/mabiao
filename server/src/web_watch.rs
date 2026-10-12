//! 调试构建下监视管理网页源码，写进 `MABIAO_WEB_DIR`。发行构建不走这里。

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

const BUILD_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug)]
struct WatchError(String);

impl fmt::Display for WatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for WatchError {}

fn err(message: impl Into<String>) -> WatchError {
    WatchError(message.into())
}

/// 上一级同时有 `package.json` 和 `vite.config.ts` 才是本仓库的网页项目。
pub fn web_project_dir(web_dir: &Path) -> Option<PathBuf> {
    let project = web_dir.parent()?;
    if project.join("package.json").is_file() && project.join("vite.config.ts").is_file() {
        Some(project.to_path_buf())
    } else {
        None
    }
}

/// 只有调试构建监视。`MABIAO_WEB_WATCH=0` 关掉，用来核对一份不再变动的 dist。
pub fn should_watch(debug: bool, watch_env: Option<&str>) -> bool {
    debug && watch_env != Some("0")
}

pub struct WebWatch {
    child: Option<Child>,
}

impl Drop for WebWatch {
    fn drop(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        stop_group(child);
    }
}

/// 需要监视时启动 `vite build --watch`，并等到这次构建写完 `index.html`。
/// 监视启动失败但目录里已有页面时，退回托管现有文件。
pub fn start(web_dir: &Path) -> Result<WebWatch, Box<dyn Error>> {
    let watch_env = std::env::var("MABIAO_WEB_WATCH").ok();
    if !should_watch(cfg!(debug_assertions), watch_env.as_deref()) {
        return Ok(WebWatch { child: None });
    }
    let Some(project) = web_project_dir(web_dir) else {
        return Ok(WebWatch { child: None });
    };

    let index = web_dir.join("index.html");
    let before = std::fs::metadata(&index)
        .and_then(|meta| meta.modified())
        .ok();
    let child = match spawn(&project) {
        Ok(child) => child,
        Err(error) => {
            if index.is_file() {
                tracing::warn!(%error, "管理网页监视没启动，继续托管现有目录");
                return Ok(WebWatch { child: None });
            }
            return Err(err(format!(
                "管理网页监视没启动，目录里也没有 index.html：{error}"
            ))
            .into());
        }
    };
    let mut watch = WebWatch { child: Some(child) };
    tracing::info!(dir = %project.display(), "正在按源码构建管理网页");
    let Some(child) = watch.child.as_mut() else {
        return Err(err("监视进程丢失").into());
    };
    wait_until_rebuilt(&index, before, child)?;
    tracing::info!("管理网页已构建。之后保存源码会自动再构建，刷新浏览器即可");
    Ok(watch)
}

fn spawn(project: &Path) -> std::io::Result<Child> {
    let mut cmd = Command::new("pnpm");
    cmd.current_dir(project)
        .args(["exec", "vite", "build", "--watch"])
        .stdin(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // 独立进程组，退出时按组停掉，避免只杀掉 pnpm、留下 vite。
        cmd.process_group(0);
    }
    cmd.spawn()
}

fn wait_until_rebuilt(
    index: &Path,
    before: Option<SystemTime>,
    child: &mut Child,
) -> Result<(), WatchError> {
    let deadline = Instant::now() + BUILD_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Err(err(format!("管理网页构建退出了：{status}"))),
            Ok(None) => {}
            Err(error) => return Err(err(format!("查看网页构建状态失败：{error}"))),
        }
        if rebuilt(index, before) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(err("管理网页构建超时（120 秒）"));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn rebuilt(index: &Path, before: Option<SystemTime>) -> bool {
    let Ok(modified) = std::fs::metadata(index).and_then(|meta| meta.modified()) else {
        return false;
    };
    match before {
        Some(prev) => modified > prev,
        None => true,
    }
}

fn stop_group(child: &mut Child) {
    match child.try_wait() {
        Ok(Some(_)) | Err(_) => return,
        Ok(None) => {}
    }
    #[cfg(unix)]
    {
        signal_group(child.id(), SIGTERM);
        let started = Instant::now();
        loop {
            if child.try_wait().ok().flatten().is_some() {
                return;
            }
            if started.elapsed() > Duration::from_secs(2) {
                signal_group(child.id(), SIGKILL);
                let _ = child.wait();
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[cfg(unix)]
const SIGTERM: i32 = 15;
#[cfg(unix)]
const SIGKILL: i32 = 9;

#[cfg(unix)]
unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

#[cfg(unix)]
fn signal_group(pid: u32, sig: i32) {
    unsafe {
        kill(-(pid as i32), sig);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_is_debug_only_and_can_be_turned_off() {
        assert!(!should_watch(false, None));
        assert!(should_watch(true, None));
        assert!(!should_watch(true, Some("0")));
        assert!(should_watch(true, Some("1")));
    }

    #[test]
    fn project_dir_requires_vite_beside_the_web_dir() {
        let root = std::env::temp_dir().join(format!(
            "mabiao-web-watch-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let dist = root.join("dist");
        std::fs::create_dir_all(&dist).unwrap();
        assert!(web_project_dir(&dist).is_none());
        std::fs::write(root.join("package.json"), "{}").unwrap();
        std::fs::write(root.join("vite.config.ts"), "export {}\n").unwrap();
        assert_eq!(web_project_dir(&dist).as_deref(), Some(root.as_path()));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn rebuilt_waits_for_a_newer_index() {
        let dir = std::env::temp_dir().join(format!(
            "mabiao-web-watch-index-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let index = dir.join("index.html");
        assert!(!rebuilt(&index, None));
        std::fs::write(&index, "old").unwrap();
        let before = std::fs::metadata(&index).unwrap().modified().unwrap();
        assert!(!rebuilt(&index, Some(before)));
        assert!(rebuilt(&index, None));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
