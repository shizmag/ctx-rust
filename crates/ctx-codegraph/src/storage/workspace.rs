use crate::error::CodeGraphError;
use std::path::{Path, PathBuf};

pub fn find_workspace_root(start_dir: &Path) -> PathBuf {
    let mut current = match start_dir.canonicalize() {
        Ok(path) => path,
        Err(_) => start_dir.to_path_buf(),
    };
    let registry = crate::backend::global_registry();
    loop {
        let mut matches = current.join(".git").exists() || current.join(".ctxconfig").exists();
        if !matches {
            for backend in registry.all() {
                for marker in backend.workspace_markers() {
                    match marker {
                        crate::backend::WorkspaceMarker::File(name) => {
                            if current.join(name).exists() {
                                matches = true;
                                break;
                            }
                        }
                        crate::backend::WorkspaceMarker::Directory(name) => {
                            if current.join(name).exists() {
                                matches = true;
                                break;
                            }
                        }
                    }
                }
                if matches {
                    break;
                }
            }
        }
        if matches {
            return current;
        }
        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => break,
        }
    }
    start_dir.to_path_buf()
}
pub fn open_codegraph_db(root: &Path) -> Result<rusqlite::Connection, CodeGraphError> {
    let workspace_root = find_workspace_root(root);
    let db_dir = workspace_root.join(".ctx-codegraph");
    std::fs::create_dir_all(&db_dir)?;
    let db_path = db_dir.join("codegraph.sqlite");
    let conn = rusqlite::Connection::open(db_path)?;
    conn.execute("PRAGMA foreign_keys = ON;", [])?;
    Ok(conn)
}

pub fn open_db(root: &Path) -> Result<rusqlite::Connection, CodeGraphError> {
    open_codegraph_db(root)
}
