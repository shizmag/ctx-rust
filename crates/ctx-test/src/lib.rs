use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct FileCoverage {
    pub covered: usize,
    pub coverable: usize,
    pub line_hits: HashMap<usize, usize>,
}

pub struct TestContext {
    // Maps normalized absolute path to detailed coverage info
    pub coverage: HashMap<PathBuf, FileCoverage>,
}

impl TestContext {
    pub fn discover(root: &Path) -> Self {
        let mut coverage_files = Vec::new();
        discover_coverage_files(root, &mut coverage_files);

        let mut coverage = HashMap::new();
        for path in coverage_files {
            if let Ok(map) = parse_coverage_file(&path, root) {
                coverage.extend(map);
            }
        }

        Self { coverage }
    }

    pub fn get_file_coverage(&self, file_path: &Path) -> Option<(usize, usize)> {
        let path = if let Ok(canon) = file_path.canonicalize() {
            canon
        } else {
            file_path.to_path_buf()
        };
        self.coverage
            .get(&path)
            .map(|fc| (fc.covered, fc.coverable))
    }

    pub fn get_file_line_coverage(&self, file_path: &Path) -> Option<HashMap<usize, usize>> {
        let path = if let Ok(canon) = file_path.canonicalize() {
            canon
        } else {
            file_path.to_path_buf()
        };
        self.coverage.get(&path).map(|fc| fc.line_hits.clone())
    }
}

pub fn count_tests(path: &Path, content: &str) -> usize {
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    match extension {
        "rs" => count_rust_tests(content),
        "py"
            if is_python_test_file(path) => {
                count_python_tests(content)
            }
        _ => 0,
    }
}

fn discover_coverage_files(root: &Path, files: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(file_type) = entry.file_type() {
                if file_type.is_dir() {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if name == ".git"
                        || name == "node_modules"
                        || name == ".venv"
                        || name == "venv"
                        || name == "__pycache__"
                        || name == ".pytest_cache"
                    {
                        continue;
                    }
                    if name == "target" {
                        let cov_path = path.join("cov");
                        if cov_path.exists() {
                            discover_coverage_files(&cov_path, files);
                        }
                        let lcov = path.join("lcov.info");
                        if lcov.exists() {
                            files.push(lcov);
                        }
                        continue;
                    }
                    discover_coverage_files(&path, files);
                } else if file_type.is_file() {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if name == "lcov.info" || name == "coverage.xml" || name == "cobertura.xml" {
                        files.push(path);
                    }
                }
            }
        }
    }
}

pub fn parse_coverage_file(
    path: &Path,
    root: &Path,
) -> Result<HashMap<PathBuf, FileCoverage>, std::io::Error> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name == "lcov.info" {
        parse_lcov(path, root)
    } else if name == "coverage.xml" || name == "cobertura.xml" {
        parse_cobertura(path, root)
    } else {
        Ok(HashMap::new())
    }
}

fn parse_lcov(path: &Path, root: &Path) -> Result<HashMap<PathBuf, FileCoverage>, std::io::Error> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut map = HashMap::new();

    let mut current_file: Option<PathBuf> = None;
    let mut line_hits = HashMap::new();

    for line_result in reader.lines() {
        let line = line_result?;
        let trimmed = line.trim();

        if let Some(filename) = trimmed.strip_prefix("SF:") {
            let file_path = if Path::new(filename).is_absolute() {
                PathBuf::from(filename)
            } else {
                root.join(filename)
            };
            if let Ok(canon) = file_path.canonicalize() {
                current_file = Some(canon);
            } else {
                current_file = Some(file_path);
            }
            line_hits.clear();
        } else if let Some(after_da) = trimmed.strip_prefix("DA:") {
            if current_file.is_some() {
                let parts: Vec<&str> = after_da.split(',').collect();
                if parts.len() >= 2
                    && let Ok(line_num) = parts[0].parse::<usize>()
                        && let Ok(hits) = parts[1].parse::<usize>() {
                            line_hits.insert(line_num, hits);
                        }
            }
        } else if trimmed == "end_of_record"
            && let Some(file_path) = current_file.take()
                && !line_hits.is_empty() {
                    let coverable = line_hits.len();
                    let covered = line_hits.values().filter(|&&h| h > 0).count();
                    map.insert(
                        file_path,
                        FileCoverage {
                            covered,
                            coverable,
                            line_hits: line_hits.clone(),
                        },
                    );
                }
    }
    Ok(map)
}

fn parse_cobertura(
    path: &Path,
    root: &Path,
) -> Result<HashMap<PathBuf, FileCoverage>, std::io::Error> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut map = HashMap::new();

    let mut current_file: Option<PathBuf> = None;
    let mut line_hits = HashMap::new();

    for line_result in reader.lines() {
        let line = line_result?;
        let trimmed = line.trim();

        if trimmed.contains("<class ") {
            if let Some(filename) = extract_attribute(trimmed, "filename") {
                let file_path = if Path::new(&filename).is_absolute() {
                    PathBuf::from(&filename)
                } else {
                    root.join(&filename)
                };
                if let Ok(canon) = file_path.canonicalize() {
                    current_file = Some(canon);
                } else {
                    current_file = Some(file_path);
                }
                line_hits.clear();
            }
        } else if trimmed.contains("<line ") {
            if current_file.is_some()
                && let (Some(num_str), Some(hits_str)) = (
                    extract_attribute(trimmed, "number"),
                    extract_attribute(trimmed, "hits"),
                )
                    && let (Ok(line_num), Ok(hits)) =
                        (num_str.parse::<usize>(), hits_str.parse::<usize>())
                    {
                        line_hits.insert(line_num, hits);
                    }
        } else if trimmed.contains("</class>")
            && let Some(file_path) = current_file.take()
                && !line_hits.is_empty() {
                    let coverable = line_hits.len();
                    let covered = line_hits.values().filter(|&&h| h > 0).count();
                    map.insert(
                        file_path,
                        FileCoverage {
                            covered,
                            coverable,
                            line_hits: line_hits.clone(),
                        },
                    );
                }
    }
    Ok(map)
}

fn extract_attribute(line: &str, attr: &str) -> Option<String> {
    let search = format!("{}=\"", attr);
    if let Some(start_idx) = line.find(&search) {
        let val_start = start_idx + search.len();
        if let Some(end_idx) = line[val_start..].find('"') {
            return Some(line[val_start..val_start + end_idx].to_string());
        }
    }
    None
}

fn count_rust_tests(content: &str) -> usize {
    let mut count = 0;
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix('#').map(|s| s.trim_start())
            && let Some(attr) = rest.strip_prefix('[').map(|s| s.trim_start())
        {
            let is_test_attr = if attr.starts_with("tokio::test") {
                    let next = attr.as_bytes().get(11);
                    next.is_none()
                        || *next.unwrap() == b']'
                        || *next.unwrap() == b'('
                        || next.unwrap().is_ascii_whitespace()
                } else if attr.starts_with("async_std::test") {
                    let next = attr.as_bytes().get(15);
                    next.is_none()
                        || *next.unwrap() == b']'
                        || *next.unwrap() == b'('
                        || next.unwrap().is_ascii_whitespace()
                } else if attr.starts_with("rstest") {
                    let next = attr.as_bytes().get(6);
                    next.is_none()
                        || *next.unwrap() == b']'
                        || *next.unwrap() == b'('
                        || next.unwrap().is_ascii_whitespace()
                } else if attr.starts_with("test_case") {
                    let next = attr.as_bytes().get(9);
                    next.is_none()
                        || *next.unwrap() == b']'
                        || *next.unwrap() == b'('
                        || next.unwrap().is_ascii_whitespace()
                } else if attr.starts_with("actix_rt::test") {
                    let next = attr.as_bytes().get(14);
                    next.is_none()
                        || *next.unwrap() == b']'
                        || *next.unwrap() == b'('
                        || next.unwrap().is_ascii_whitespace()
                } else if attr.starts_with("test") {
                    let next = attr.as_bytes().get(4);
                    next.is_none()
                        || *next.unwrap() == b']'
                        || *next.unwrap() == b'('
                        || next.unwrap().is_ascii_whitespace()
                } else {
                    false
                };
                if is_test_attr {
                    count += 1;
                }
        }
    }
    count
}

fn is_python_test_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    (name.starts_with("test_") && name.ends_with(".py")) || name.ends_with("_test.py")
}

fn count_python_tests(content: &str) -> usize {
    let mut count = 0;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("def test_") {
            count += 1;
        }
    }
    count
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_rust_tests() {
        let content = "
            #[test]
            fn a() {}
            #[tokio::test]
            async fn b() {}
            #[cfg(test)]
            mod tests {}
            #[test_case(1)]
            fn c() {}
        ";
        assert_eq!(count_rust_tests(content), 3);
    }

    #[test]
    fn test_count_python_tests() {
        let content = "
            def test_one():
                pass
            class TestClass:
                def test_method(self):
                    pass
            def helper():
                pass
        ";
        assert_eq!(count_python_tests(content), 2);
    }
}
