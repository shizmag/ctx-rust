//! ML research export: JSON features, GraphML call graph.

use std::path::Path;

use ctx_codegraph_lang::CodeGraphError;
use rusqlite::Connection;
use serde::Serialize;

/// Symbol-level feature row for ML dataset export.
#[derive(Debug, Serialize)]
pub struct SymbolFeatureRow {
    pub id: i64,
    pub file_path: String,
    pub name: String,
    pub qualified_name: String,
    pub kind: String,
    pub language: String,
    pub lines_of_code: i64,
    pub nesting_depth: i64,
    pub complexity_proxy: i64,
    pub param_count: i64,
    pub fan_in: i64,
    pub fan_out: i64,
    pub coupling: f64,
    pub cohesion: f64,
    pub parent_symbol_id: Option<i64>,
}

/// Module-level aggregated metrics for ML export.
#[derive(Debug, Serialize)]
pub struct ModuleFeatureRow {
    pub module_path: String,
    pub total_loc: i64,
    pub symbol_count: i64,
    pub avg_complexity: f64,
    pub avg_nesting_depth: f64,
    pub call_density: f64,
}

/// Complete feature dataset export payload.
#[derive(Debug, Serialize)]
pub struct FeatureExport {
    pub schema_version: String,
    pub extraction_tier: Option<String>,
    pub symbols: Vec<SymbolFeatureRow>,
    pub module_metrics: Vec<ModuleFeatureRow>,
}

/// Export symbol and module metrics as JSON for ML pipelines.
pub fn export_features_json(conn: &Connection) -> Result<String, CodeGraphError> {
    let schema_version: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| "unknown".to_string());

    let extraction_tier: Option<String> = conn
        .query_row(
            "SELECT value FROM metadata WHERE key = 'extraction_tier'",
            [],
            |row| row.get(0),
        )
        .ok();

    let mut stmt = conn.prepare(
        "SELECT s.id, f.path, s.name, s.qualified_name, s.kind, s.language,
                s.lines_of_code, s.nesting_depth, s.complexity_proxy, s.param_count,
                s.fan_in, s.fan_out, s.coupling, s.cohesion, s.parent_symbol_id
         FROM symbols s
         JOIN files f ON s.file_id = f.id
         ORDER BY s.id",
    )?;

    let symbols: Vec<SymbolFeatureRow> = stmt
        .query_map([], |row| {
            Ok(SymbolFeatureRow {
                id: row.get(0)?,
                file_path: row.get(1)?,
                name: row.get(2)?,
                qualified_name: row.get(3)?,
                kind: row.get(4)?,
                language: row.get(5)?,
                lines_of_code: row.get(6)?,
                nesting_depth: row.get(7)?,
                complexity_proxy: row.get(8)?,
                param_count: row.get(9)?,
                fan_in: row.get(10)?,
                fan_out: row.get(11)?,
                coupling: row.get(12)?,
                cohesion: row.get(13)?,
                parent_symbol_id: row.get(14)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut mod_stmt = conn.prepare(
        "SELECT module_path, total_loc, symbol_count, avg_complexity, avg_nesting_depth, call_density
         FROM module_metrics ORDER BY module_path",
    )?;
    let module_metrics: Vec<ModuleFeatureRow> = mod_stmt
        .query_map([], |row| {
            Ok(ModuleFeatureRow {
                module_path: row.get(0)?,
                total_loc: row.get(1)?,
                symbol_count: row.get(2)?,
                avg_complexity: row.get(3)?,
                avg_nesting_depth: row.get(4)?,
                call_density: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let export = FeatureExport {
        schema_version,
        extraction_tier,
        symbols,
        module_metrics,
    };

    serde_json::to_string_pretty(&export)
        .map_err(|e| CodeGraphError::Internal(format!("JSON export failed: {e}")))
}

/// Export call graph edges as GraphML for graph analysis tools.
pub fn export_call_graph_graphml(conn: &Connection) -> Result<String, CodeGraphError> {
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<graphml xmlns="http://graphml.graphdrawing.org/xmlns">
  <graph edgedefault="directed">
"#,
    );

    let mut node_stmt = conn.prepare(
        "SELECT id, qualified_name, kind FROM symbols ORDER BY id",
    )?;
    let nodes = node_stmt.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
    })?;
    for node in nodes {
        let (id, qname, kind) = node?;
        xml.push_str(&format!(
            r#"    <node id="n{id}"><data key="label">{qname}</data><data key="kind">{kind}</data></node>
"#
        ));
    }

    let mut edge_stmt = conn.prepare(
        "SELECT from_symbol_id, to_symbol_id, confidence
         FROM edges WHERE kind = 'Call' AND from_symbol_id IS NOT NULL AND to_symbol_id IS NOT NULL",
    )?;
    let edges = edge_stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    for (i, edge) in edges.enumerate() {
        let (from_id, to_id, confidence) = edge?;
        xml.push_str(&format!(
            r#"    <edge id="e{i}" source="n{from_id}" target="n{to_id}"><data key="confidence">{confidence}</data></edge>
"#
        ));
    }

    xml.push_str("  </graph>\n</graphml>\n");
    Ok(xml)
}

/// Write feature JSON export to a file path.
pub fn export_features_json_to_file(
    conn: &Connection,
    output: &Path,
) -> Result<(), CodeGraphError> {
    let json = export_features_json(conn)?;
    std::fs::write(output, json)
        .map_err(|e| CodeGraphError::Internal(format!("write export failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctx_codegraph_lang::backend::BackendRegistry;
    use crate::storage::schema::init_schema;

    #[test]
    fn export_features_json_on_empty_db() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.sqlite");
        let conn = Connection::open(&db_path).unwrap();
        let registry = BackendRegistry::new();
        init_schema(&conn, &registry).unwrap();

        let json = export_features_json(&conn).unwrap();
        assert!(json.contains("\"symbols\""));
        assert!(json.contains("\"module_metrics\""));
    }
}