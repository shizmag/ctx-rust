use crate::error::CodeGraphError;

pub fn init_schema(conn: &rusqlite::Connection) -> Result<(), CodeGraphError> {
    let meta_exists: bool = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='metadata'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count > 0)
        .unwrap_or(false);

    let mut needs_drop = false;
    if meta_exists {
        let schema_version: Option<String> = conn
            .query_row(
                "SELECT value FROM metadata WHERE key = 'schema_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .ok();
        if schema_version.as_deref() != Some("4") {
            needs_drop = true;
        }
    }

    if needs_drop {
        let tables = vec![
            "metadata",
            "files",
            "symbols",
            "call_sites",
            "call_edges",
            "occurrences",
            "edges",
        ];
        for table in tables {
            let _ = conn.execute(&format!("DROP TABLE IF EXISTS {}", table), []);
        }
    }

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS metadata (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS files (
            id INTEGER PRIMARY KEY,
            path TEXT NOT NULL UNIQUE,
            rel_path TEXT NOT NULL,
            language TEXT NOT NULL,
            backend_id TEXT NOT NULL,
            mtime_ms INTEGER NOT NULL,
            size_bytes INTEGER NOT NULL,
            content_hash TEXT,
            parser_id TEXT NOT NULL,
            parser_version TEXT NOT NULL,
            parser_config_hash TEXT NOT NULL,
            indexed_at_ms INTEGER,
            parse_status TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS symbols (
            id INTEGER PRIMARY KEY,
            file_id INTEGER NOT NULL,
            name TEXT NOT NULL,
            qualified_name TEXT NOT NULL,
            kind TEXT NOT NULL,
            language TEXT NOT NULL,
            start_line INTEGER NOT NULL,
            start_col INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            end_col INTEGER NOT NULL,
            body_start_line INTEGER,
            body_start_col INTEGER,
            body_end_line INTEGER,
            body_end_col INTEGER,
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS occurrences (
            id INTEGER PRIMARY KEY,
            file_id INTEGER NOT NULL,
            enclosing_symbol_id INTEGER,
            kind TEXT NOT NULL,
            raw_text TEXT NOT NULL,
            start_line INTEGER NOT NULL,
            start_col INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            end_col INTEGER NOT NULL,
            language TEXT NOT NULL,
            backend_id TEXT NOT NULL,
            FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE,
            FOREIGN KEY(enclosing_symbol_id) REFERENCES symbols(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS edges (
            id INTEGER PRIMARY KEY,
            kind TEXT NOT NULL,
            from_file_id INTEGER NOT NULL,
            from_symbol_id INTEGER,
            to_symbol_id INTEGER,
            to_external TEXT,
            occurrence_id INTEGER,
            raw_text TEXT,
            start_line INTEGER,
            start_col INTEGER,
            end_line INTEGER,
            end_col INTEGER,
            confidence TEXT NOT NULL,
            produced_by TEXT,
            FOREIGN KEY(from_file_id) REFERENCES files(id) ON DELETE CASCADE,
            FOREIGN KEY(from_symbol_id) REFERENCES symbols(id) ON DELETE CASCADE,
            FOREIGN KEY(to_symbol_id) REFERENCES symbols(id) ON DELETE SET NULL,
            FOREIGN KEY(occurrence_id) REFERENCES occurrences(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_files_path ON files(path);
        CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
        CREATE INDEX IF NOT EXISTS idx_symbols_qualified_name ON symbols(qualified_name);
        CREATE INDEX IF NOT EXISTS idx_symbols_file_id ON symbols(file_id);
        CREATE INDEX IF NOT EXISTS idx_occurrences_enclosing ON occurrences(enclosing_symbol_id);
        CREATE INDEX IF NOT EXISTS idx_occurrences_raw_text ON occurrences(raw_text);
        CREATE INDEX IF NOT EXISTS idx_edges_from_symbol ON edges(from_symbol_id);
        CREATE INDEX IF NOT EXISTS idx_edges_to_symbol ON edges(to_symbol_id);
        CREATE INDEX IF NOT EXISTS idx_edges_confidence ON edges(confidence);
    ",
    )?;

    conn.execute(
        "INSERT OR REPLACE INTO metadata (key, value) VALUES ('schema_version', '4')",
        [],
    )?;
    // Store default backends metadata initially
    let registry = crate::backend::global_registry();
    let metas: Vec<_> = registry
        .all()
        .iter()
        .map(|b| b.metadata(&crate::index::BuildIndexOptions::default()))
        .collect();
    let metas_str = serde_json::to_string(&metas).unwrap_or_default();
    conn.execute(
        "INSERT OR REPLACE INTO metadata (key, value) VALUES ('backends_metadata', ?1)",
        [metas_str],
    )?;

    Ok(())
}
pub fn validate_index_invariants(conn: &rusqlite::Connection) -> Result<(), CodeGraphError> {
    let invalid_symbols: i64 = conn.query_row(
        "SELECT count(*) FROM symbols WHERE file_id NOT IN (SELECT id FROM files)",
        [],
        |row| row.get(0),
    )?;
    if invalid_symbols > 0 {
        return Err(CodeGraphError::Parse(format!(
            "Invariant violation: {} symbols with invalid file_id",
            invalid_symbols
        )));
    }

    let invalid_occurrences: i64 = conn.query_row(
        "SELECT count(*) FROM occurrences WHERE file_id NOT IN (SELECT id FROM files)",
        [],
        |row| row.get(0),
    )?;
    if invalid_occurrences > 0 {
        return Err(CodeGraphError::Parse(format!(
            "Invariant violation: {} occurrences with invalid file_id",
            invalid_occurrences
        )));
    }

    let invalid_edges_source: i64 = conn.query_row(
        "SELECT count(*) FROM edges WHERE from_symbol_id IS NOT NULL AND from_symbol_id NOT IN (SELECT id FROM symbols)",
        [],
        |row| row.get(0),
    )?;
    if invalid_edges_source > 0 {
        return Err(CodeGraphError::Parse(format!(
            "Invariant violation: {} edges with invalid source symbol id",
            invalid_edges_source
        )));
    }

    let invalid_edges_target: i64 = conn.query_row(
        "SELECT count(*) FROM edges WHERE to_symbol_id IS NOT NULL AND to_symbol_id NOT IN (SELECT id FROM symbols)",
        [],
        |row| row.get(0),
    )?;
    if invalid_edges_target > 0 {
        return Err(CodeGraphError::Parse(format!(
            "Invariant violation: {} edges pointing to non-existent symbol",
            invalid_edges_target
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::workspace::{open_db, read_metadata, write_metadata};

    #[test]
    fn test_initializes_schema() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open_db(dir.path()).unwrap();
        init_schema(&conn).unwrap();

        let mut stmt = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            )
            .unwrap();
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
        let mut tables = vec![];
        for row in rows {
            tables.push(row.unwrap());
        }

        assert!(tables.contains(&"metadata".to_string()));
        assert!(tables.contains(&"files".to_string()));
        assert!(tables.contains(&"symbols".to_string()));
        assert!(tables.contains(&"occurrences".to_string()));
        assert!(tables.contains(&"edges".to_string()));
    }

    #[test]
    fn test_read_write_metadata_helpers() {
        // Regression test for helpers used by `ctx stats` (index details) and MCP persist of mcp_last_stats.
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join(".ctx-codegraph/codegraph.sqlite");
        assert!(!db_path.exists());

        // absent -> no create, none
        assert!(read_metadata(dir.path(), "k").is_none());
        assert!(write_metadata(dir.path(), "k", "v").is_err());
        assert!(!db_path.exists());

        let conn = open_db(dir.path()).unwrap();
        init_schema(&conn).unwrap();
        assert!(db_path.exists());

        write_metadata(dir.path(), "mcp_last_stats", "{\"calls\":5}").unwrap();
        assert_eq!(read_metadata(dir.path(), "mcp_last_stats").as_deref(), Some("{\"calls\":5}"));
        write_metadata(dir.path(), "schema_version", "test").unwrap();
        assert_eq!(read_metadata(dir.path(), "schema_version").as_deref(), Some("test"));
        assert!(read_metadata(dir.path(), "missing").is_none());
    }
}
