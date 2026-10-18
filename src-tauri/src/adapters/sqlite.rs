use std::collections::BTreeSet;

use rusqlite::Connection;

pub(crate) fn table_columns(conn: &Connection, table: &str) -> Result<BTreeSet<String>, String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| error.to_string())?;
    let mut names = BTreeSet::new();
    for name in rows {
        names.insert(name.map_err(|error| error.to_string())?);
    }
    Ok(names)
}

pub(crate) fn sql_text(columns: &BTreeSet<String>, name: &str) -> String {
    if columns.contains(name) {
        format!("COALESCE({name}, '')")
    } else {
        "''".to_string()
    }
}

pub(crate) fn sql_int(columns: &BTreeSet<String>, name: &str) -> String {
    if columns.contains(name) {
        format!("COALESCE({name}, 0)")
    } else {
        "0".to_string()
    }
}

pub(crate) fn sql_real(columns: &BTreeSet<String>, name: &str) -> String {
    if columns.contains(name) {
        format!("COALESCE({name}, 0)")
    } else {
        "0".to_string()
    }
}
