use crate::source_sqlite;

#[test]
fn open_wal_readonly_reads_uncheckpointed_wal_handles_special_chars_and_blocks_writes() {
    let dir = tempfile::tempdir().unwrap();
    let special_dir = dir.path().join("space dir#1?q%20");
    std::fs::create_dir_all(&special_dir).unwrap();
    let db_path = special_dir.join("client.db");

    let writer = rusqlite::Connection::open(&db_path).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    writer.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    writer
        .execute_batch(
            "CREATE TABLE items (id INTEGER PRIMARY KEY, label TEXT);
             INSERT INTO items VALUES (1, 'committed');",
        )
        .unwrap();
    writer
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .unwrap();

    // 新增一行且保持写连接打开，确保停留在 -wal 中。
    writer
        .execute("INSERT INTO items VALUES (2, 'wal-only')", [])
        .unwrap();
    assert!(source_sqlite::sqlite_sidecar_path(&db_path, "-wal").exists());

    let reader = source_sqlite::open_wal_readonly(&db_path).unwrap();
    let count: i64 = reader
        .query_row("SELECT COUNT(*) FROM items", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        count, 2,
        "open_wal_readonly 必须读到未 checkpoint 的 WAL 帧"
    );

    assert!(
        reader
            .execute("INSERT INTO items VALUES (3, 'nope')", [])
            .is_err(),
        "open_wal_readonly 必须启用 query_only 拒绝写入"
    );
}

#[test]
fn open_plain_readonly_reads_rows_and_blocks_writes() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("plain.db");

    let writer = rusqlite::Connection::open(&db_path).unwrap();
    writer
        .execute_batch(
            "CREATE TABLE kv (k TEXT PRIMARY KEY, v TEXT);
             INSERT INTO kv VALUES ('mode', 'plain');",
        )
        .unwrap();
    drop(writer);

    let reader = source_sqlite::open_plain_readonly(&db_path).unwrap();
    let value: String = reader
        .query_row("SELECT v FROM kv WHERE k = 'mode'", [], |row| row.get(0))
        .unwrap();
    assert_eq!(value, "plain");
    assert!(
        reader
            .execute("INSERT INTO kv VALUES ('x', 'y')", [])
            .is_err(),
        "open_plain_readonly 必须启用 query_only 拒绝写入"
    );
}

#[test]
fn wal_fingerprint_and_revision_track_wal_and_exclude_shm() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("tracked.db");
    let writer = rusqlite::Connection::open(&db_path).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    writer.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    writer
        .execute_batch("CREATE TABLE logs (id INTEGER PRIMARY KEY, body TEXT);")
        .unwrap();

    let fp_before = source_sqlite::wal_fingerprint(&db_path);
    let rev_before = source_sqlite::wal_source_revision(&db_path, "Test").unwrap();

    // 真实只读打开 + 修改 -shm 都不应改变指纹或修订号。
    let reader = source_sqlite::open_wal_readonly(&db_path).unwrap();
    let _: i64 = reader
        .query_row("SELECT COUNT(*) FROM logs", [], |row| row.get(0))
        .unwrap();
    drop(reader);

    let shm_path = source_sqlite::sqlite_sidecar_path(&db_path, "-shm");
    let shm_bytes = std::fs::read(&shm_path).unwrap_or_default();
    std::fs::write(&shm_path, &shm_bytes).unwrap();
    assert_eq!(source_sqlite::wal_fingerprint(&db_path), fp_before);
    assert_eq!(
        source_sqlite::wal_source_revision(&db_path, "Test").unwrap(),
        rev_before
    );

    // 未 checkpoint 的 -wal 写入必须立刻改变指纹和修订号。
    writer
        .execute("INSERT INTO logs VALUES (1, 'fresh-wal-frame')", [])
        .unwrap();
    assert_ne!(source_sqlite::wal_fingerprint(&db_path), fp_before);
    assert_ne!(
        source_sqlite::wal_source_revision(&db_path, "Test").unwrap(),
        rev_before
    );
}

#[test]
fn schema_introspection_and_typed_row_readers_handle_missing_and_null_columns() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE records (
            id TEXT PRIMARY KEY,
            title TEXT,
            payload BLOB,
            score INTEGER,
            cost REAL
         );
         INSERT INTO records VALUES ('r1', NULL, X'6869', NULL, 1.5);
         INSERT INTO records VALUES ('r2', 'hello', 'text-bytes', 42, NULL);",
    )
    .unwrap();

    let tables = source_sqlite::table_names(&conn).unwrap();
    assert!(tables.contains("records"));
    assert!(source_sqlite::table_exists(&conn, "records"));
    assert!(!source_sqlite::table_exists(&conn, "missing_table"));

    let cols = source_sqlite::table_columns(&conn, "records").unwrap();
    assert!(source_sqlite::column_exists(&conn, "records", "title"));
    assert!(!source_sqlite::column_exists(
        &conn,
        "records",
        "missing_col"
    ));
    assert!(source_sqlite::table_columns(&conn, "records; DROP TABLE records").is_err());

    let sql = format!(
        "SELECT {id}, {title}, {missing_text}, {score}, {missing_int}, {cost}, {missing_real}, {nullable_cost}, {aliased_missing}, {fallback_col}, payload
         FROM records AS r
         ORDER BY id",
        id = source_sqlite::sql_text(&cols, "id"),
        title = source_sqlite::sql_text(&cols, "title"),
        missing_text = source_sqlite::sql_text(&cols, "nonexistent_text"),
        score = source_sqlite::sql_int(&cols, "score"),
        missing_int = source_sqlite::sql_int(&cols, "nonexistent_int"),
        cost = source_sqlite::sql_real(&cols, "cost"),
        missing_real = source_sqlite::sql_real(&cols, "nonexistent_real"),
        nullable_cost = source_sqlite::sql_nullable(&cols, "cost"),
        aliased_missing = source_sqlite::sql_aliased_nullable(&cols, "r", "nonexistent_opt"),
        fallback_col = source_sqlite::sql_fallback(&cols, "nonexistent_fb", "NULL"),
    );

    let mut stmt = conn.prepare(&sql).unwrap();
    let rows: Vec<_> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, f64>(5)?,
                row.get::<_, f64>(6)?,
                row.get::<_, Option<f64>>(7)?,
                row.get::<_, Option<String>>(8)?,
                source_sqlite::row_opt_string(row, 9)?,
                source_sqlite::row_blob_or_text(row, 10)?,
                source_sqlite::row_text_lossy(row, 3)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, "r1");
    assert_eq!(rows[0].1, "");
    assert_eq!(rows[0].2, "");
    assert_eq!(rows[0].3, 0);
    assert_eq!(rows[0].4, 0);
    assert_eq!(rows[0].5, 1.5);
    assert_eq!(rows[0].6, 0.0);
    assert_eq!(rows[0].7, Some(1.5));
    assert_eq!(rows[0].8, None);
    assert_eq!(rows[0].9, "");
    assert_eq!(rows[0].10, b"hi");
    assert_eq!(rows[0].11, "0");

    assert_eq!(rows[1].0, "r2");
    assert_eq!(rows[1].1, "hello");
    assert_eq!(rows[1].3, 42);
    assert_eq!(rows[1].7, None);
    assert_eq!(rows[1].10, b"text-bytes");
    assert_eq!(rows[1].11, "42");
}
