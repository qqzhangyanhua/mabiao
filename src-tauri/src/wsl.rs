//! Windows 下只读发现 WSL 发行版用户家目录，作为额外扫描根。
//!
//! 未安装 WSL、UNC 枚举失败或超时都静默跳过，不拖慢摄取。非 Windows 平台恒为空。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[cfg(test)]
use std::cell::RefCell;

#[cfg(windows)]
const SHARE_TIMEOUT: Duration = Duration::from_millis(400);

#[cfg(test)]
thread_local! {
    static TEST_EXTRA_HOMES: RefCell<Option<Vec<PathBuf>>> = const { RefCell::new(None) };
}

/// 本机 Windows 家目录之外，还要当作「家」扫一遍的路径（WSL 发行版用户目录）。
pub(crate) fn extra_scan_homes() -> Vec<PathBuf> {
    #[cfg(test)]
    {
        if let Some(homes) = TEST_EXTRA_HOMES.with(|slot| slot.borrow().clone()) {
            return homes;
        }
    }
    #[cfg(windows)]
    {
        return discover_wsl_homes();
    }
    #[cfg(not(windows))]
    Vec::new()
}

/// `home` 加上 WSL 发行版用户目录。设置页/环境变量覆盖某一来源时不要用这个列表替换覆盖值。
pub(crate) fn scan_homes(home: &Path) -> Vec<PathBuf> {
    let mut homes = vec![home.to_path_buf()];
    for extra in extra_scan_homes() {
        if extra != home && !homes.contains(&extra) {
            homes.push(extra);
        }
    }
    homes
}

pub(crate) fn unc_share_candidates() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        vec![PathBuf::from(r"\\wsl.localhost"), PathBuf::from(r"\\wsl$")]
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// 从已经列出来的 WSL 共享根（`\\wsl.localhost` / 测试夹具）收集各发行版 `/home/<user>`。
pub(crate) fn homes_from_share_roots(share_roots: &[PathBuf]) -> Vec<PathBuf> {
    homes_from_share_roots_timed(share_roots, None)
}

fn homes_from_share_roots_timed(
    share_roots: &[PathBuf],
    timeout: Option<Duration>,
) -> Vec<PathBuf> {
    let mut homes = Vec::new();
    for share in share_roots {
        for distro in read_dirs(share, timeout) {
            for user in read_dirs(&distro.join("home"), timeout) {
                let name = user
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("");
                if name.is_empty() || name.starts_with('.') {
                    continue;
                }
                if !homes.contains(&user) {
                    homes.push(user);
                }
            }
        }
    }
    homes
}

#[cfg(windows)]
fn discover_wsl_homes() -> Vec<PathBuf> {
    homes_from_share_roots_timed(&unc_share_candidates(), Some(SHARE_TIMEOUT))
}

fn read_dirs(path: &Path, timeout: Option<Duration>) -> Vec<PathBuf> {
    match timeout {
        Some(limit) => read_dirs_timeout(path, limit),
        None => collect_dir_entries(path),
    }
}

fn collect_dir_entries(path: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(path) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
        .map(|entry| entry.path())
        .collect()
}

fn read_dirs_timeout(path: &Path, timeout: Duration) -> Vec<PathBuf> {
    let path = path.to_path_buf();
    let (tx, rx) = mpsc::channel();
    if thread::Builder::new()
        .name("wsl-home-scan".into())
        .spawn(move || {
            let _ = tx.send(collect_dir_entries(&path));
        })
        .is_err()
    {
        return Vec::new();
    }
    rx.recv_timeout(timeout).unwrap_or_default()
}

#[cfg(test)]
pub(crate) fn with_extra_scan_homes<T>(homes: Vec<PathBuf>, f: impl FnOnce() -> T) -> T {
    TEST_EXTRA_HOMES.with(|slot| *slot.borrow_mut() = Some(homes));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    TEST_EXTRA_HOMES.with(|slot| *slot.borrow_mut() = None);
    match result {
        Ok(value) => value,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}
