use std::path::Path;

pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

pub fn get_relative_path(path: &Path, root_path: &Path) -> String {
    match path.strip_prefix(root_path) {
        Ok(rel) => rel.to_string_lossy().to_string(),
        Err(_) => path.to_string_lossy().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1024.0 MB");
    }

    #[test]
    fn test_get_relative_path() {
        let root = Path::new("/workspace/project");
        
        // relative path inside root
        let path1 = Path::new("/workspace/project/src/lib.rs");
        assert_eq!(get_relative_path(path1, root), "src/lib.rs");

        // path outside root
        let path2 = Path::new("/other/place/file.txt");
        assert_eq!(get_relative_path(path2, root), "/other/place/file.txt");
    }
}
