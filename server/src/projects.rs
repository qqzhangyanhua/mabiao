//! 项目归并：优先按 git remote，没有就按目录名兜底，同时保留各设备上的原始路径。

use sqlx::{PgConnection, Row};

use crate::error::AppError;

/// 归并键与展示名。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectKey {
    pub key: String,
    pub name: String,
    /// 归一后的 remote（`host/owner/repo`）；目录兜底时为空。
    pub git_remote: Option<String>,
}

/// 把 `git@github.com:Owner/Repo.git`、`https://user:token@github.com/owner/repo/`、
/// `ssh://git@github.com:22/owner/repo.git` 都归一成 `github.com/owner/repo`。
///
/// 凭证、协议、端口、大小写、`.git` 后缀都不进结果。本地路径形式的 remote
/// （`/srv/git/x.git`、`file://…`）在别人机器上没有意义，返回 `None` 交给目录兜底。
pub fn normalize_git_remote(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let (host, path) = if let Some((scheme, rest)) = raw.split_once("://") {
        if scheme.eq_ignore_ascii_case("file") {
            return None;
        }
        let (authority, path) = rest.split_once('/')?;
        let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        let host = host_port.split(':').next()?;
        (host, path)
    } else {
        // scp 形式：[user@]host:path
        let (left, path) = raw.split_once(':')?;
        let host = left.rsplit_once('@').map_or(left, |(_, h)| h);
        if host.len() < 2 || host.contains(['/', '\\']) {
            return None;
        }
        (host, path.trim_start_matches('/'))
    };

    let path = path
        .trim_matches('/')
        .trim_end_matches(".git")
        .trim_matches('/');
    if host.is_empty() || path.is_empty() {
        return None;
    }
    Some(format!("{}/{}", host.to_lowercase(), path.to_lowercase()))
}

fn base_name(path: &str) -> Option<&str> {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
}

/// 推送载荷里的 remote 与目录路径 → 归并键。remote 优先，没有或不可用就用目录名。
pub fn project_key(project_path: &str, git_remote: Option<&str>) -> Option<ProjectKey> {
    if let Some(remote) = git_remote.and_then(normalize_git_remote) {
        let name = remote.rsplit('/').next().unwrap_or(&remote).to_owned();
        return Some(ProjectKey {
            key: format!("git:{remote}"),
            name,
            git_remote: Some(remote),
        });
    }
    base_name(project_path).map(|name| ProjectKey {
        key: format!("dir:{}", name.to_lowercase()),
        name: name.to_owned(),
        git_remote: None,
    })
}

async fn find_or_create(conn: &mut PgConnection, key: &ProjectKey) -> Result<i64, AppError> {
    // 冲突时做一次空更新只为拿到 id；不能覆盖管理员以后改过的名字。
    let row = sqlx::query(
        "INSERT INTO projects (key, name, git_remote) VALUES ($1, $2, $3)
         ON CONFLICT (key) DO UPDATE SET key = EXCLUDED.key
         RETURNING id",
    )
    .bind(&key.key)
    .bind(&key.name)
    .bind(&key.git_remote)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row.get("id"))
}

/// 把该设备上的某个路径指向项目。路径原来只有目录兜底项目时，把挂在该路径下的会话与消耗记录一并改指过去：
/// 先推了消耗记录（只有目录兜底）、后推会话（带 remote）的顺序不该让历史记录留在错的项目里。
async fn point_path_at(
    conn: &mut PgConnection,
    device_pk: i64,
    path: &str,
    project_id: i64,
) -> Result<(), AppError> {
    let previous: Option<(i64, Option<String>)> = sqlx::query_as(
        "SELECT pp.project_id, p.git_remote
         FROM project_paths pp JOIN projects p ON p.id = pp.project_id
         WHERE pp.device_pk = $1 AND pp.path = $2 FOR UPDATE OF pp",
    )
    .bind(device_pk)
    .bind(path)
    .fetch_optional(&mut *conn)
    .await?;
    if previous.as_ref().is_some_and(|(id, _)| *id == project_id) {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO project_paths (device_pk, path, project_id) VALUES ($1, $2, $3)
         ON CONFLICT (device_pk, path) DO UPDATE SET project_id = EXCLUDED.project_id",
    )
    .bind(device_pk)
    .bind(path)
    .bind(project_id)
    .execute(&mut *conn)
    .await?;
    // 只改挂目录兜底项目下的历史。路径换了 remote（重新 clone、改了仓库地址）时，
    // 旧历史仍属于旧的 git 项目，不跟着漂。
    if let Some((previous, None)) = previous {
        for table in ["sessions", "usage_records"] {
            sqlx::query(&format!(
                "UPDATE {table} SET project_id = $1
                 WHERE device_pk = $2 AND project_path = $3 AND project_id = $4"
            ))
            .bind(project_id)
            .bind(device_pk)
            .bind(path)
            .bind(previous)
            .execute(&mut *conn)
            .await?;
        }
    }
    Ok(())
}

/// 给一次推送里的（路径, remote）找到或建出项目，并登记路径映射。
///
/// - 有可用 remote：归到 git 项目，路径改指它。
/// - 没有 remote：路径已有映射就沿用（不把已归到 git 项目的路径降级成目录兜底）；
///   没有映射才按目录名兜底并登记。
/// - 路径为空且没有 remote：没有可归并的线索，返回 `None`。
pub async fn resolve(
    conn: &mut PgConnection,
    device_pk: i64,
    project_path: &str,
    git_remote: Option<&str>,
) -> Result<Option<i64>, AppError> {
    let Some(key) = project_key(project_path, git_remote) else {
        return Ok(None);
    };
    if key.git_remote.is_some() {
        let id = find_or_create(conn, &key).await?;
        if !project_path.is_empty() {
            point_path_at(conn, device_pk, project_path, id).await?;
        }
        return Ok(Some(id));
    }
    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT project_id FROM project_paths WHERE device_pk = $1 AND path = $2",
    )
    .bind(device_pk)
    .bind(project_path)
    .fetch_optional(&mut *conn)
    .await?;
    if existing.is_some() {
        return Ok(existing);
    }
    let id = find_or_create(conn, &key).await?;
    point_path_at(conn, device_pk, project_path, id).await?;
    Ok(Some(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_common_remote_spelling_normalizes_to_the_same_value() {
        for raw in [
            "git@github.com:Owner/Repo.git",
            "https://github.com/owner/repo.git",
            "https://github.com/owner/repo",
            "https://github.com/owner/repo/",
            "https://user:s3cret-token@github.com/owner/repo.git",
            "ssh://git@github.com:22/owner/repo.git",
            "git://GitHub.com/Owner/Repo.git",
            "  git@github.com:owner/repo.git\n",
        ] {
            assert_eq!(
                normalize_git_remote(raw).as_deref(),
                Some("github.com/owner/repo"),
                "{raw}"
            );
        }
    }

    #[test]
    fn credentials_never_survive_normalization() {
        let normalized =
            normalize_git_remote("https://oauth2:glpat-SECRET@gitlab.example.com/team/app.git")
                .unwrap();
        assert_eq!(normalized, "gitlab.example.com/team/app");
        assert!(!normalized.contains("SECRET"));
    }

    #[test]
    fn nested_group_paths_are_kept() {
        assert_eq!(
            normalize_git_remote("git@gitlab.com:group/sub/app.git").as_deref(),
            Some("gitlab.com/group/sub/app")
        );
    }

    #[test]
    fn unusable_remotes_are_none() {
        for raw in [
            "",
            "   ",
            "/srv/git/repo.git",
            "file:///srv/git/repo.git",
            "C:\\work\\repo",
            "not a url",
            "https://github.com",
            "https://github.com/",
        ] {
            assert_eq!(normalize_git_remote(raw), None, "{raw:?}");
        }
    }

    #[test]
    fn remote_wins_over_directory_name() {
        let key =
            project_key("/Users/alice/work/whatever", Some("git@github.com:o/r.git")).unwrap();
        assert_eq!(key.key, "git:github.com/o/r");
        assert_eq!(key.name, "r");
        assert_eq!(key.git_remote.as_deref(), Some("github.com/o/r"));
    }

    #[test]
    fn directory_name_is_the_fallback_on_either_path_style() {
        for path in [
            "/Users/alice/work/MaBiao",
            "C:\\work\\MaBiao\\",
            "/x/MaBiao/",
        ] {
            let key = project_key(path, None).unwrap();
            assert_eq!(key.key, "dir:mabiao", "{path}");
            assert_eq!(key.name, "MaBiao");
            assert_eq!(key.git_remote, None);
        }
        let unusable_remote = project_key("/a/app", Some("/srv/git/app.git")).unwrap();
        assert_eq!(unusable_remote.key, "dir:app");
    }

    #[test]
    fn no_path_and_no_remote_means_no_project() {
        assert_eq!(project_key("", None), None);
        assert_eq!(project_key("/", None), None);
    }
}
