use crate::{conversation, ingest, user_files};
use tauri::Manager;

use crate::domain::{
    ConversationAttachmentContentDto, ConversationDetailDto, ConversationDetailStateDto,
    ConversationEventAnchor, ConversationEventContentDto, ConversationEventPage,
    ConversationExportFormat, ConversationIndexProgressDto, ConversationPage, ConversationQuery,
    ConversationUsagePage,
};
use crate::AppState;

#[tauri::command]
pub async fn get_conversation_sessions_page(
    app: tauri::AppHandle,
    query: ConversationQuery,
) -> Result<ConversationPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let prices = state.effective_prices();
        conversation::sessions_page_with_prices(&conn, &query, &prices)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_conversation_tool_names(
    app: tauri::AppHandle,
    query: ConversationQuery,
) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        conversation::catalog_tool_names(&conn, &query)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_conversation_detail(
    app: tauri::AppHandle,
    source: String,
    session_id: String,
) -> Result<ConversationDetailDto, String> {
    let home = ingest::default_home();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let read = conversation::prepare_detail_read(&conn, &home, &source, &session_id)?;
        drop(conn);
        conversation::finish_prepared_detail(&home, read)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_conversation_events(
    app: tauri::AppHandle,
    source: String,
    session_id: String,
    anchor: ConversationEventAnchor,
    limit: Option<u32>,
) -> Result<ConversationEventPage, String> {
    let home = ingest::default_home();
    let limit = limit.unwrap_or(200);
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        let read =
            conversation::prepare_events_read(&conn, &home, &source, &session_id, &anchor, limit)?;
        drop(conn);
        conversation::finish_prepared_events(&home, read, &anchor, limit)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_conversation_index_progress(
    app: tauri::AppHandle,
) -> Result<ConversationIndexProgressDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        conversation::event_index_progress(&conn)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_conversation_usage_records(
    app: tauri::AppHandle,
    source: String,
    session_id: String,
    page: u32,
    page_size: u32,
) -> Result<ConversationUsagePage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        conversation::usage_records_page(&conn, &source, &session_id, page, page_size)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_conversation_detail_state(
    app: tauri::AppHandle,
    source: String,
    session_id: String,
    known_revision: String,
) -> Result<ConversationDetailStateDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        conversation::detail_state(
            &conn,
            &ingest::default_home(),
            &source,
            &session_id,
            &known_revision,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_conversation_event_content(
    app: tauri::AppHandle,
    source: String,
    session_id: String,
    event_id: String,
) -> Result<ConversationEventContentDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        conversation::load_event_content(
            &conn,
            &ingest::default_home(),
            &source,
            &session_id,
            &event_id,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_conversation_attachment(
    app: tauri::AppHandle,
    source: String,
    session_id: String,
    attachment_id: String,
) -> Result<ConversationAttachmentContentDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        conversation::load_attachment(
            &conn,
            &ingest::default_home(),
            &source,
            &session_id,
            &attachment_id,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_conversation_attachment_thumbnail(
    app: tauri::AppHandle,
    source: String,
    session_id: String,
    attachment_id: String,
) -> Result<ConversationAttachmentContentDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let conn = state.lock_read()?;
        conversation::load_attachment_thumbnail(
            &conn,
            &ingest::default_home(),
            &source,
            &session_id,
            &attachment_id,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn export_conversation(
    app: tauri::AppHandle,
    source: String,
    session_id: String,
    format: ConversationExportFormat,
) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let home = ingest::default_home();
        let default_name = {
            let state = app.state::<AppState>();
            let conn = state.lock_read()?;
            conversation::export_default_name(&conn, &home, &source, &session_id, format)?
        };
        let (label, extensions): (&str, &[&str]) = match format {
            ConversationExportFormat::Markdown => ("Markdown", &["md"]),
            ConversationExportFormat::Json => ("Raw JSON", &["jsonl"]),
        };
        let path = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter(label, extensions)
            .save_file();
        match path {
            Some(path) => {
                let expected_mtime = user_files::observe_mtime(&path)?;
                let state = app.state::<AppState>();
                let conn = state.lock_read()?;
                conversation::write_conversation_export(
                    &conn,
                    &home,
                    &source,
                    &session_id,
                    format,
                    &path,
                    expected_mtime.as_deref(),
                )?;
                Ok(true)
            }
            None => Ok(false),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
