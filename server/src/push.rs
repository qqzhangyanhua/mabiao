//! 推送接收：单场会话整场覆盖，消耗记录按指纹去重（ADR 0026）。

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use push_protocol::{
    check_protocol_version, DeviceInfo, PushSessionRequest, PushSessionResponse, PushUsageRequest,
    PushUsageResponse, SessionPayload, UsageRecordPayload,
};

use crate::auth::AuthedAccount;
use crate::error::{ApiJson, AppError};
use crate::sessions::parse_timestamp;
use crate::usage::PreparedUsage;
use crate::{devices, projects, sessions, team_pricing, usage, AppState};

/// 单次请求里最多多少条消耗记录。客户端按会话分批，超了说明客户端有问题。
pub const MAX_USAGE_RECORDS_PER_REQUEST: usize = 5000;
/// 推送请求体上限。一场长会话的事件正文可以很大，远超默认的 2 MiB。
pub const PUSH_BODY_LIMIT_BYTES: usize = 64 * 1024 * 1024;

const MAX_ID_CHARS: usize = 512;
const MAX_DEVICE_NAME_CHARS: usize = 256;

fn check_text(name: &str, value: &str, max_chars: usize) -> Result<(), AppError> {
    if value.is_empty() {
        return Err(AppError::invalid(format!("{name} 不能为空")));
    }
    if value.chars().count() > max_chars {
        return Err(AppError::invalid(format!("{name} 最多 {max_chars} 个字符")));
    }
    Ok(())
}

fn check_device(device: &DeviceInfo) -> Result<(), AppError> {
    check_text("device_id", &device.device_id, MAX_ID_CHARS)?;
    check_text("device_name", &device.device_name, MAX_DEVICE_NAME_CHARS)
}

fn check_session(session: &SessionPayload) -> Result<(), AppError> {
    check_text("source", &session.source, MAX_ID_CHARS)?;
    check_text("session_id", &session.session_id, MAX_ID_CHARS)
}

fn check_usage_record(record: &UsageRecordPayload) -> Result<(), AppError> {
    check_text("fingerprint", &record.fingerprint, MAX_ID_CHARS)?;
    check_text("source", &record.source, MAX_ID_CHARS)?;
    let t = &record.tokens;
    if [
        t.input,
        t.output,
        t.cache_read,
        t.cache_creation,
        t.reasoning,
        t.total,
    ]
    .iter()
    .any(|n| *n < 0)
    {
        return Err(AppError::invalid("token 数不能为负"));
    }
    Ok(())
}

/// 推一场会话。账号取自 token，不取自请求体：成员只能写进自己名下。
pub async fn push_session(
    State(state): State<AppState>,
    caller: AuthedAccount,
    ApiJson(request): ApiJson<PushSessionRequest>,
) -> Result<Json<PushSessionResponse>, AppError> {
    request.validate()?;
    check_device(&request.device)?;
    check_session(&request.session)?;
    let session = &request.session;

    let mut tx = state.pool.begin().await?;
    let device_pk = devices::upsert(&mut *tx, caller.id, &request.device).await?;
    let project_id = projects::resolve(
        &mut tx,
        device_pk,
        &session.project,
        session.git_remote_url.as_deref(),
    )
    .await?;
    let (_, replaced) =
        sessions::upsert(&mut tx, caller.id, device_pk, session, project_id).await?;
    tx.commit().await?;

    Ok(Json(PushSessionResponse {
        source: session.source.clone(),
        session_id: session.session_id.clone(),
        replaced,
    }))
}

/// 推一批消耗记录。同一批里任何一条不合法就整批拒收，不做半截入库。
pub async fn push_usage(
    State(state): State<AppState>,
    caller: AuthedAccount,
    ApiJson(request): ApiJson<PushUsageRequest>,
) -> Result<Json<PushUsageResponse>, AppError> {
    check_protocol_version(request.protocol_version)?;
    check_device(&request.device)?;
    if request.records.len() > MAX_USAGE_RECORDS_PER_REQUEST {
        return Err(AppError::invalid(format!(
            "单次最多 {MAX_USAGE_RECORDS_PER_REQUEST} 条消耗记录"
        )));
    }
    let mut occurred_at = Vec::with_capacity(request.records.len());
    for record in &request.records {
        check_usage_record(record)?;
        occurred_at.push(
            parse_timestamp(&record.occurred_at)
                .ok_or_else(|| AppError::invalid("消耗记录的 occurred_at 不是 RFC 3339 时间"))?,
        );
    }

    let mut tx = state.pool.begin().await?;
    team_pricing::lock_shared(&mut tx).await?;
    let table = team_pricing::load_table(&mut tx).await?;
    let device_pk = devices::upsert(&mut *tx, caller.id, &request.device).await?;

    // 消耗记录只带目录，没有 remote；同一路径只解析一次。
    let mut project_by_path: HashMap<&str, Option<i64>> = HashMap::new();
    let mut prepared = Vec::with_capacity(request.records.len());
    for (record, occurred_at) in request.records.iter().zip(occurred_at) {
        let project_id = match project_by_path.get(record.project.as_str()) {
            Some(known) => *known,
            None => {
                let resolved = projects::resolve(&mut tx, device_pk, &record.project, None).await?;
                project_by_path.insert(&record.project, resolved);
                resolved
            }
        };
        prepared.push(PreparedUsage {
            record,
            occurred_at,
            project_id,
        });
    }

    let inserted = usage::insert_new(&mut tx, caller.id, device_pk, &prepared, &table).await?;
    tx.commit().await?;

    let total = request.records.len() as u64;
    Ok(Json(PushUsageResponse {
        inserted: inserted as u32,
        duplicates: (total - inserted) as u32,
    }))
}

/// 成员删自己的会话，管理员删任意。别人的会话得到 403；不存在得到 404。
pub async fn delete_session(
    State(state): State<AppState>,
    caller: AuthedAccount,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let owner = sessions::owner_of(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::not_found("会话不存在"))?;
    if !caller.can_access(owner) {
        return Err(AppError::forbidden("只能删除自己推送的会话"));
    }
    sessions::delete(&state.pool, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_fields_must_be_non_empty_and_bounded() {
        assert!(check_text("x", "ok", 4).is_ok());
        assert!(check_text("x", "", 4).is_err());
        assert!(check_text("x", "12345", 4).is_err());
        assert!(check_text("x", "四个字符啊", 5).is_ok());
    }
}
