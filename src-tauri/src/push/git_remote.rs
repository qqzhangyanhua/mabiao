//! 项目归并线索：只读会话工作目录的 `.git/config`，取 remote URL（ADR 0026「推什么」）。
//!
//! 只读，不写用户文件，ADR 0010 不受影响。读不到一律返回 `None`，不报错：没有 remote 的目录
//! 本来就多，服务端会按目录名兜底。

use std::fs;
use std::path::{Path, PathBuf};

/// `origin` 优先，没有 origin 取第一个有 `url` 的 remote。
pub fn git_remote_url(project: &str) -> Option<String> {
    let project = project.trim();
    if project.is_empty() {
        return None;
    }
    let config = git_config_path(Path::new(project))?;
    remote_url_from_config(&fs::read_to_string(config).ok()?).map(|url| strip_userinfo(&url))
}

/// `https://user:token@host/repo.git` 里的凭据不该离开本机，服务端归并时也不要它。
/// scp 写法（`git@host:org/repo`）的 `git@` 只是用户名，不是凭据，原样保留。
fn strip_userinfo(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    match authority.rsplit_once('@') {
        Some((_, host)) => format!("{scheme}://{host}{tail}"),
        None => url.to_string(),
    }
}

/// 普通仓库 `.git` 是目录；worktree / submodule 的 `.git` 是一个 `gitdir: …` 文件，
/// 配置在共用目录（worktree 经 `commondir` 指过去）。
fn git_config_path(project: &Path) -> Option<PathBuf> {
    let dot_git = project.join(".git");
    let metadata = fs::metadata(&dot_git).ok()?;
    if metadata.is_dir() {
        return Some(dot_git.join("config"));
    }
    let pointer = fs::read_to_string(&dot_git).ok()?;
    let target = pointer.trim().strip_prefix("gitdir:")?.trim();
    let git_dir = resolve(project, target);
    let common = fs::read_to_string(git_dir.join("commondir"))
        .ok()
        .map(|relative| resolve(&git_dir, relative.trim()))
        .unwrap_or(git_dir);
    Some(common.join("config"))
}

fn resolve(base: &Path, target: &str) -> PathBuf {
    let target = Path::new(target);
    if target.is_absolute() {
        target.to_path_buf()
    } else {
        base.join(target)
    }
}

fn remote_url_from_config(config: &str) -> Option<String> {
    let mut current_remote: Option<String> = None;
    let mut first: Option<String> = None;
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            current_remote = remote_section_name(line);
            continue;
        }
        let Some(name) = &current_remote else {
            continue;
        };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        if key.trim() != "url" || value.is_empty() {
            continue;
        }
        if name == "origin" {
            return Some(value.to_string());
        }
        first.get_or_insert_with(|| value.to_string());
    }
    first
}

/// `[remote "origin"]` → `origin`。
fn remote_section_name(header: &str) -> Option<String> {
    let inner = header.strip_prefix('[')?.split(']').next()?.trim();
    let rest = inner.strip_prefix("remote")?.trim();
    let name = rest.strip_prefix('"')?.strip_suffix('"')?;
    Some(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_wins_over_other_remotes() {
        let config = "[core]\n\turl = nope\n[remote \"upstream\"]\n\turl = https://example.com/up.git\n[remote \"origin\"]\n\turl = git@github.com:acme/app.git\n";
        assert_eq!(
            remote_url_from_config(config).as_deref(),
            Some("git@github.com:acme/app.git")
        );
    }

    #[test]
    fn credentials_in_url_are_dropped_but_scp_user_stays() {
        assert_eq!(
            strip_userinfo("https://alice:ghp_secret@github.com/acme/app.git"),
            "https://github.com/acme/app.git"
        );
        assert_eq!(
            strip_userinfo("ssh://git@github.com:22/acme/app.git"),
            "ssh://github.com:22/acme/app.git"
        );
        assert_eq!(
            strip_userinfo("git@github.com:acme/app.git"),
            "git@github.com:acme/app.git"
        );
        assert_eq!(
            strip_userinfo("https://github.com/acme/a@b.git"),
            "https://github.com/acme/a@b.git"
        );
    }

    #[test]
    fn falls_back_to_first_remote_and_ignores_non_remote_sections() {
        let config = "[branch \"main\"]\n\turl = nope\n[remote \"fork\"]\n\turl = https://example.com/fork.git\n";
        assert_eq!(
            remote_url_from_config(config).as_deref(),
            Some("https://example.com/fork.git")
        );
        assert_eq!(remote_url_from_config("[core]\n\tbare = false\n"), None);
    }
}
