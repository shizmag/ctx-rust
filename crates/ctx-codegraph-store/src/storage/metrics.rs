//! Tier 2 graph metrics: fan-in/fan-out, coupling, LCOM, module aggregations.

use std::path::Path;

use ctx_codegraph_lang::CodeGraphError;
use rusqlite::Transaction;

/// Compute and persist all Tier 2 graph metrics in a single transaction.
pub fn compute_and_save_graph_metrics_in_tx(
    tx: &Transaction<'_>,
) -> Result<(), CodeGraphError> {
    tx.execute(
        "UPDATE symbols SET 
            fan_in = (SELECT COUNT(*) FROM edges WHERE to_symbol_id = symbols.id AND kind = 'Call'),
            fan_out = (SELECT COUNT(*) FROM edges WHERE from_symbol_id = symbols.id AND kind = 'Call')",
        [],
    )?;

    tx.execute(
        "UPDATE symbols SET coupling = (
            SELECT COUNT(DISTINCT to_sym.qualified_name)
            FROM edges
            JOIN symbols AS to_sym ON edges.to_symbol_id = to_sym.id
            WHERE edges.from_symbol_id = symbols.id
              AND to_sym.kind IN ('Class', 'Struct', 'Module', 'Trait', 'Impl')
              AND to_sym.id != symbols.id
        )",
        [],
    )?;

    compute_and_update_lcom(tx)?;
    compute_module_aggregations(tx)?;

    Ok(())
}

fn compute_and_update_lcom(tx: &Transaction<'_>) -> Result<(), CodeGraphError> {
    let mut stmt = tx.prepare("SELECT id FROM symbols WHERE kind IN ('Class', 'Struct')")?;
    let class_ids: Vec<i64> = stmt
        .query_map([], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    drop(stmt);

    let mut update_stmt = tx.prepare("UPDATE symbols SET cohesion = ?1 WHERE id = ?2")?;

    for class_id in class_ids {
        let mut stmt =
            tx.prepare("SELECT id FROM symbols WHERE parent_symbol_id = ?1 AND kind = 'Method'")?;
        let method_ids: Vec<i64> = stmt
            .query_map([class_id], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        drop(stmt);

        if method_ids.len() <= 1 {
            update_stmt.execute(rusqlite::params![0.0, class_id])?;
            continue;
        }

        let mut method_refs = Vec::new();
        for &method_id in &method_ids {
            let mut stmt = tx.prepare(
                "SELECT DISTINCT raw_text FROM occurrences WHERE enclosing_symbol_id = ?1",
            )?;
            let refs: std::collections::HashSet<String> = stmt
                .query_map([method_id], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            method_refs.push(refs);
        }

        let mut p = 0;
        let mut q = 0;
        for i in 0..method_ids.len() {
            for j in i + 1..method_ids.len() {
                let overlap = method_refs[i].intersection(&method_refs[j]).count();
                if overlap == 0 {
                    p += 1;
                } else {
                    q += 1;
                }
            }
        }

        let lcom = (p - q).max(0) as f64;
        update_stmt.execute(rusqlite::params![lcom, class_id])?;
    }
    Ok(())
}

fn compute_module_aggregations(tx: &Transaction<'_>) -> Result<(), CodeGraphError> {
    tx.execute("DELETE FROM module_metrics", [])?;

    struct DirMetrics {
        total_loc: i64,
        symbol_count: i64,
        total_complexity: i64,
        total_nesting_depth: i64,
        call_count: i64,
    }

    let mut dir_map: std::collections::HashMap<String, DirMetrics> =
        std::collections::HashMap::new();

    let mut stmt = tx.prepare(
        "SELECT files.rel_path, symbols.lines_of_code, symbols.complexity_proxy, symbols.nesting_depth 
         FROM symbols 
         JOIN files ON symbols.file_id = files.id",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let rel_path_str: String = row.get(0)?;
        let loc: i64 = row.get(1)?;
        let complexity: i64 = row.get(2)?;
        let nesting: i64 = row.get(3)?;

        let path = Path::new(&rel_path_str);
        let mut current_dir = path.parent();
        while let Some(dir) = current_dir {
            let dir_str = dir.to_string_lossy().to_string();
            if dir_str.is_empty() {
                break;
            }
            let entry = dir_map.entry(dir_str).or_insert(DirMetrics {
                total_loc: 0,
                symbol_count: 0,
                total_complexity: 0,
                total_nesting_depth: 0,
                call_count: 0,
            });
            entry.total_loc += loc;
            entry.symbol_count += 1;
            entry.total_complexity += complexity;
            entry.total_nesting_depth += nesting;
            current_dir = dir.parent();
        }
    }

    let mut stmt = tx.prepare(
        "SELECT files.rel_path 
         FROM occurrences 
         JOIN files ON occurrences.file_id = files.id 
         WHERE occurrences.kind = 'Call'",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let rel_path_str: String = row.get(0)?;
        let path = Path::new(&rel_path_str);
        let mut current_dir = path.parent();
        while let Some(dir) = current_dir {
            let dir_str = dir.to_string_lossy().to_string();
            if dir_str.is_empty() {
                break;
            }
            if let Some(entry) = dir_map.get_mut(&dir_str) {
                entry.call_count += 1;
            }
            current_dir = dir.parent();
        }
    }

    let mut insert_stmt = tx.prepare(
        "INSERT OR REPLACE INTO module_metrics (
            module_path, total_loc, symbol_count, avg_complexity, avg_nesting_depth, call_density
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;

    for (dir_str, metrics) in dir_map {
        let avg_complexity = if metrics.symbol_count > 0 {
            metrics.total_complexity as f64 / metrics.symbol_count as f64
        } else {
            0.0
        };
        let avg_nesting_depth = if metrics.symbol_count > 0 {
            metrics.total_nesting_depth as f64 / metrics.symbol_count as f64
        } else {
            0.0
        };
        let call_density = if metrics.total_loc > 0 {
            metrics.call_count as f64 / metrics.total_loc as f64
        } else {
            0.0
        };

        insert_stmt.execute(rusqlite::params![
            dir_str,
            metrics.total_loc,
            metrics.symbol_count,
            avg_complexity,
            avg_nesting_depth,
            call_density,
        ])?;
    }

    Ok(())
}