use crate::test_support::*;

#[test]
fn backup_and_restore_round_trips_records_and_user_config() {
    let root = tempfile::tempdir().unwrap();
    let live = root.path().join("live");
    let dest = root.path().join("backup");
    std::fs::create_dir_all(&live).unwrap();

    let db_path = live.join("usage.sqlite");
    let prices_path = live.join("prices.json");
    let snapshot_path = live.join("litellm_prices.json");
    let budget_path = live.join("budget.json");
    let budget_notify_path = live.join("budget_notify_state.json");
    let paths = backup::AppDataPaths {
        db_path: db_path.clone(),
        prices_path: prices_path.clone(),
        snapshot_path: snapshot_path.clone(),
        budget_path: budget_path.clone(),
        budget_notify_path: budget_notify_path.clone(),
        official_quota_path: live.join("official_quota.json"),
        official_quota_notify_path: live.join("official_quota_notify_state.json"),
    };

    let conn = store::open_db(db_path.to_str().unwrap()).unwrap();
    store::insert_records(
        &conn,
        &[rec(
            "2026-08-18T00:00:00.000Z",
            Source::Claude,
            "claude-sonnet-5",
            "anthropic",
            "/proj",
            "s1",
            42,
        )],
    )
    .unwrap();

    let prices = PriceTable {
        prices: vec![PriceEntry {
            model: "claude-sonnet-5".into(),
            provider: Some("anthropic".into()),
            input: 0.003,
            output: 0.015,
            cache_read: 0.0,
            cache_creation: 0.0,
            origin: PriceOrigin::User,
        }],
    };
    std::fs::write(&prices_path, serde_json::to_string_pretty(&prices).unwrap()).unwrap();
    budget::save_config(
        &budget_path,
        &BudgetConfig {
            monthly_usd: Some(20.0),
        },
    )
    .unwrap();
    budget::save_notify_state(
        &budget_notify_path,
        &budget::NotifyState {
            month: "2026-08".into(),
            notified: vec![50, 80],
        },
    )
    .unwrap();
    std::fs::write(
        &snapshot_path,
        r#"{"as_of":"2026-01-01","source":"test","entries":[]}"#,
    )
    .unwrap();

    let manifest = backup::backup_to(&conn, &dest, &paths).unwrap();
    assert!(manifest.files.contains(&"usage.sqlite".to_string()));
    assert!(manifest.files.contains(&"prices.json".to_string()));
    assert!(manifest.files.contains(&"budget.json".to_string()));
    assert!(manifest
        .files
        .contains(&"budget_notify_state.json".to_string()));
    assert!(manifest.note.contains("钥匙串"));
    drop(conn);

    std::fs::write(&prices_path, "{\"prices\":[]}").unwrap();
    budget::save_config(&budget_path, &BudgetConfig { monthly_usd: None }).unwrap();
    budget::save_notify_state(&budget_notify_path, &budget::NotifyState::default()).unwrap();
    std::fs::remove_file(&db_path).unwrap();
    let _ = std::fs::remove_file(live.join("usage.sqlite-wal"));
    let _ = std::fs::remove_file(live.join("usage.sqlite-shm"));

    backup::restore_from(&dest, &paths).unwrap();
    let restored = store::open_db(db_path.to_str().unwrap()).unwrap();
    let rows = store::load_all(&restored).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].total_tokens, 42);
    assert_eq!(budget::load_config(&budget_path).monthly_usd, Some(20.0));
    assert_eq!(
        budget::load_notify_state(&budget_notify_path),
        budget::NotifyState {
            month: "2026-08".into(),
            notified: vec![50, 80],
        }
    );
    let restored_prices: PriceTable =
        serde_json::from_str(&std::fs::read_to_string(&prices_path).unwrap()).unwrap();
    assert_eq!(restored_prices.prices[0].model, "claude-sonnet-5");
}

#[test]
fn restore_rejects_invalid_backup_without_touching_live_files() {
    let root = tempfile::tempdir().unwrap();
    let live = root.path().join("live");
    let dest = root.path().join("backup");
    std::fs::create_dir_all(&live).unwrap();
    std::fs::create_dir_all(&dest).unwrap();

    let db_path = live.join("usage.sqlite");
    let prices_path = live.join("prices.json");
    let snapshot_path = live.join("litellm_prices.json");
    let budget_path = live.join("budget.json");
    let paths = backup::AppDataPaths {
        db_path: db_path.clone(),
        prices_path: prices_path.clone(),
        snapshot_path,
        budget_path,
        budget_notify_path: live.join("budget_notify_state.json"),
        official_quota_path: live.join("official_quota.json"),
        official_quota_notify_path: live.join("official_quota_notify_state.json"),
    };

    let conn = store::open_db(db_path.to_str().unwrap()).unwrap();
    store::insert_records(
        &conn,
        &[rec(
            "2026-08-18T00:00:00.000Z",
            Source::Claude,
            "claude-sonnet-5",
            "anthropic",
            "/proj",
            "s1",
            42,
        )],
    )
    .unwrap();
    drop(conn);
    std::fs::write(&prices_path, "{\"prices\":[]}").unwrap();

    assert!(backup::validate_restore(&dest).is_err());
    assert!(backup::restore_from(&dest, &paths).is_err());

    std::fs::write(
        dest.join("manifest.json"),
        "{\"created_at\":\"x\",\"files\":[],\"note\":\"\"}",
    )
    .unwrap();
    assert!(
        backup::restore_from(&dest, &paths)
            .unwrap_err()
            .contains("usage.sqlite"),
        "missing sqlite should fail before overwrite"
    );

    let still = store::open_db(db_path.to_str().unwrap()).unwrap();
    assert_eq!(store::load_all(&still).unwrap()[0].total_tokens, 42);
    assert_eq!(
        std::fs::read_to_string(&prices_path).unwrap(),
        "{\"prices\":[]}"
    );
}

#[test]
fn restore_rolls_back_live_files_when_a_later_replace_fails() {
    let root = tempfile::tempdir().unwrap();
    let live = root.path().join("live");
    let dest = root.path().join("backup");
    std::fs::create_dir_all(&live).unwrap();

    let db_path = live.join("usage.sqlite");
    let prices_path = live.join("prices.json");
    let snapshot_path = live.join("litellm_prices.json");
    let budget_path = live.join("budget.json");
    let paths = backup::AppDataPaths {
        db_path: db_path.clone(),
        prices_path: prices_path.clone(),
        snapshot_path,
        budget_path,
        budget_notify_path: live.join("budget_notify_state.json"),
        official_quota_path: live.join("official_quota.json"),
        official_quota_notify_path: live.join("official_quota_notify_state.json"),
    };

    let conn = store::open_db(db_path.to_str().unwrap()).unwrap();
    store::insert_records(
        &conn,
        &[rec(
            "2026-08-18T00:00:00.000Z",
            Source::Claude,
            "claude-sonnet-5",
            "anthropic",
            "/proj",
            "s1",
            42,
        )],
    )
    .unwrap();
    std::fs::write(&prices_path, "{\"prices\":[]}").unwrap();
    backup::backup_to(&conn, &dest, &paths).unwrap();
    drop(conn);

    std::fs::remove_file(&prices_path).unwrap();
    std::fs::create_dir(&prices_path).unwrap();

    let error = backup::restore_from(&dest, &paths).unwrap_err();
    assert!(error.contains("写入") || error.contains("失败"), "{error}");

    let still = store::open_db(db_path.to_str().unwrap()).unwrap();
    assert_eq!(
        store::load_all(&still).unwrap()[0].total_tokens,
        42,
        "db should roll back when a later file cannot be replaced"
    );
}

fn backup_paths(live: &std::path::Path) -> backup::AppDataPaths {
    backup::AppDataPaths {
        db_path: live.join("usage.sqlite"),
        prices_path: live.join("prices.json"),
        snapshot_path: live.join("litellm_prices.json"),
        budget_path: live.join("budget.json"),
        budget_notify_path: live.join("budget_notify_state.json"),
        official_quota_path: live.join("official_quota.json"),
        official_quota_notify_path: live.join("official_quota_notify_state.json"),
    }
}

#[test]
fn backup_omits_conversation_event_bodies_and_restore_reads_via_fallback() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let live = root.path().join("live");
    let dest = root.path().join("backup");
    std::fs::create_dir_all(&live).unwrap();
    write_home_fixture(
        &home,
        ".codex/sessions/2026/08/rollout-conv-1.jsonl",
        "codex-conversation.jsonl",
    );
    let paths = backup_paths(&live);
    let conn = store::open_db(paths.db_path.to_str().unwrap()).unwrap();
    crate::conversation::refresh_codex(&conn, &home).unwrap();
    let live_events = crate::conversation::indexed_events(&conn, "codex", "conv-1").unwrap();
    assert!(!live_events.is_empty());
    assert!(live_events
        .iter()
        .any(|event| event.text.as_deref() == Some("我先检查现有实现。")));

    let manifest = backup::backup_to(&conn, &dest, &paths).unwrap();
    assert!(manifest.note.contains("对话"));
    drop(conn);

    let backup_db = rusqlite::Connection::open(dest.join(backup::DB_NAME)).unwrap();
    let has_events_table: bool = backup_db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'conversation_events')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!has_events_table, "备份产物不得包含事件索引表");
    let has_fts_table: bool = backup_db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'conversation_events_fts')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!has_fts_table, "备份产物不得包含正文全文索引");
    let generations: i64 = backup_db
        .query_row(
            "SELECT COUNT(*) FROM conversation_sessions WHERE event_index_generation IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(generations, 0, "备份不得留下可被当成已索引的代次");
    let raw = std::fs::read(dest.join(backup::DB_NAME)).unwrap();
    let raw = String::from_utf8_lossy(&raw);
    assert!(
        !raw.contains("我先检查现有实现。"),
        "VACUUM 后备份文件不得残留对话正文"
    );

    std::fs::remove_file(&paths.db_path).unwrap();
    let _ = std::fs::remove_file(live.join("usage.sqlite-wal"));
    let _ = std::fs::remove_file(live.join("usage.sqlite-shm"));
    backup::restore_from(&dest, &paths).unwrap();

    let restored = store::open_db(paths.db_path.to_str().unwrap()).unwrap();
    assert!(
        crate::conversation::indexed_events(&restored, "codex", "conv-1")
            .unwrap()
            .is_empty()
    );
    let fallback = crate::conversation::load_events(
        &restored,
        &home,
        "codex",
        "conv-1",
        crate::domain::ConversationEventAnchor::First,
        200,
    )
    .unwrap();
    assert!(
        fallback
            .events
            .iter()
            .any(|event| event.text.as_deref() == Some("我先检查现有实现。")),
        "恢复后未索引会话必须经回退路径读到正确内容"
    );
    assert!(fallback
        .events
        .iter()
        .any(|event| event.text.as_deref() == Some("已完成提交。")));

    crate::conversation::backfill_event_index(&restored, &home).unwrap();
    assert_conversation_index_matches_parse(&restored, &home, "codex", "conv-1");
}

#[test]
fn restore_accepts_legacy_backup_without_conversation_events_table() {
    let root = tempfile::tempdir().unwrap();
    let live = root.path().join("live");
    let dest = root.path().join("backup");
    std::fs::create_dir_all(&live).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    let paths = backup_paths(&live);

    let source = store::open_db(paths.db_path.to_str().unwrap()).unwrap();
    store::insert_records(
        &source,
        &[rec(
            "2026-08-18T00:00:00.000Z",
            Source::Claude,
            "claude-sonnet-5",
            "anthropic",
            "/proj",
            "s1",
            42,
        )],
    )
    .unwrap();
    backup::backup_to(&source, &dest, &paths).unwrap();
    drop(source);

    let backup_db = rusqlite::Connection::open(dest.join(backup::DB_NAME)).unwrap();
    backup_db
        .execute_batch("DROP TABLE IF EXISTS conversation_events;")
        .unwrap();
    drop(backup_db);

    backup::validate_restore(&dest).unwrap();
    std::fs::remove_file(&paths.db_path).unwrap();
    backup::restore_from(&dest, &paths).unwrap();
    let restored = store::open_db(paths.db_path.to_str().unwrap()).unwrap();
    assert_eq!(store::load_all(&restored).unwrap()[0].total_tokens, 42);
}
