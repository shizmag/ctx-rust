use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    pub mode: Option<String>,
    pub max_depth: Option<usize>,
    pub max_file_size: Option<u64>,
    pub exclude: Vec<String>,
}

pub fn load_config(path: &Path) -> Result<Config, std::io::Error> {
    if !path.exists() {
        return Ok(Config::default());
    }

    let content = fs::read_to_string(path)?;
    let mut config = Config::default();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };

        let key = key.trim().to_lowercase();
        let value = value.trim();

        match key.as_str() {
            "mode" => config.mode = Some(value.to_string()),
            "max_depth" => {
                if let Ok(depth) = value.parse::<usize>() {
                    config.max_depth = Some(depth);
                }
            }
            "max_file_size" => {
                if let Ok(size) = value.parse::<u64>() {
                    config.max_file_size = Some(size);
                }
            }
            "exclude" => {
                let items: Vec<String> = value
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                config.exclude.extend(items);
            }
            _ => {}
        }
    }

    Ok(config)
}

pub fn find_config(start_dir: &Path) -> Option<PathBuf> {
    let mut current = match start_dir.canonicalize() {
        Ok(path) => path,
        Err(_) => return None,
    };

    loop {
        let config_path = current.join(".ctxconfig");
        if config_path.exists() {
            return Some(config_path);
        }

        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => break,
        }
    }

    None
}

pub fn find_and_load_config(start_dir: &Path) -> Result<Config, std::io::Error> {
    if let Some(config_path) = find_config(start_dir) {
        load_config(&config_path)
    } else {
        Ok(Config::default())
    }
}
