use std::collections::HashMap;
use std::path::Path;

pub fn write_duration_cache(cwd: &Path, durations: &[(String, u64)]) {
    let cache_dir = cwd.join(".tach_cache");
    let _ = std::fs::create_dir_all(&cache_dir);
    let cache_file = cache_dir.join("durations");
    let mut lines = Vec::with_capacity(durations.len());
    for (name, ms) in durations {
        lines.push(format!("{}:{}", name, ms));
    }
    let _ = std::fs::write(&cache_file, lines.join("\n"));
}

pub fn read_duration_cache(cwd: &Path) -> HashMap<String, u64> {
    let cache_file = cwd.join(".tach_cache").join("durations");
    match std::fs::read_to_string(&cache_file) {
        Ok(content) => content
            .lines()
            .filter_map(|l| {
                let (name, ms) = l.rsplit_once(':')?;
                Some((name.to_string(), ms.parse().ok()?))
            })
            .collect(),
        Err(_) => HashMap::new(),
    }
}

pub fn write_lastfailed_cache(cwd: &Path, failed_ids: &[String]) {
    let cache_dir = cwd.join(".tach_cache");
    let _ = std::fs::create_dir_all(&cache_dir);
    let cache_file = cache_dir.join("lastfailed");
    if failed_ids.is_empty() {
        let _ = std::fs::remove_file(&cache_file);
    } else {
        let _ = std::fs::write(&cache_file, failed_ids.join("\n"));
    }
}

pub fn read_lastfailed_cache(cwd: &Path) -> Vec<String> {
    read_lastfailed_cache_from(&cwd.join(".tach_cache").join("lastfailed"))
}

pub fn read_lastfailed_cache_from(path: &Path) -> Vec<String> {
    match std::fs::read_to_string(path) {
        Ok(content) => content
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect(),
        Err(_) => Vec::new(),
    }
}

pub fn write_interrupted_cache(root: &Path, completed_ids: &[String]) {
    let cache_dir = root.join(".tach_cache");
    let _ = std::fs::create_dir_all(&cache_dir);
    let path = cache_dir.join("interrupted");
    if completed_ids.is_empty() {
        let _ = std::fs::remove_file(&path);
        return;
    }
    if let Ok(mut f) = std::fs::File::create(&path) {
        use std::io::Write;
        for id in completed_ids {
            let _ = writeln!(f, "{}", id);
        }
    }
}

pub fn read_interrupted_cache(root: &Path) -> Vec<String> {
    read_lastfailed_cache_from(&root.join(".tach_cache/interrupted"))
}

pub fn clear_interrupted_cache(root: &Path) {
    let _ = std::fs::remove_file(root.join(".tach_cache/interrupted"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_duration_cache_roundtrip() {
        let dir = TempDir::new().unwrap();
        let durations = vec![("test_a".to_string(), 100), ("test_b".to_string(), 250)];
        write_duration_cache(dir.path(), &durations);
        let loaded = read_duration_cache(dir.path());
        assert_eq!(loaded.get("test_a"), Some(&100));
        assert_eq!(loaded.get("test_b"), Some(&250));
        assert_eq!(loaded.len(), 2);
    }

    #[test]
    fn test_duration_cache_empty() {
        let dir = TempDir::new().unwrap();
        let loaded = read_duration_cache(dir.path());
        assert!(loaded.is_empty());
    }

    #[test]
    fn test_lastfailed_cache_roundtrip() {
        let dir = TempDir::new().unwrap();
        let ids = vec!["test_a".to_string(), "test_b".to_string()];
        write_lastfailed_cache(dir.path(), &ids);
        let loaded = read_lastfailed_cache(dir.path());
        assert_eq!(loaded, ids);
    }

    #[test]
    fn test_lastfailed_cache_empty_clears_file() {
        let dir = TempDir::new().unwrap();
        let ids = vec!["test_a".to_string()];
        write_lastfailed_cache(dir.path(), &ids);
        assert!(dir.path().join(".tach_cache/lastfailed").exists());

        write_lastfailed_cache(dir.path(), &[]);
        assert!(!dir.path().join(".tach_cache/lastfailed").exists());
    }

    #[test]
    fn test_lastfailed_cache_from_nonexistent() {
        let result = read_lastfailed_cache_from(Path::new("/nonexistent/path"));
        assert!(result.is_empty());
    }

    #[test]
    fn test_duration_cache_with_colons_in_name() {
        let dir = TempDir::new().unwrap();
        let durations = vec![("test_file::TestClass::test_method".to_string(), 42)];
        write_duration_cache(dir.path(), &durations);
        let loaded = read_duration_cache(dir.path());
        assert_eq!(loaded.get("test_file::TestClass::test_method"), Some(&42));
    }

    #[test]
    fn test_interrupted_cache_roundtrip() {
        let dir = TempDir::new().unwrap();
        let ids = vec!["test_a".to_string(), "test_b".to_string()];
        write_interrupted_cache(dir.path(), &ids);
        let loaded = read_interrupted_cache(dir.path());
        assert_eq!(loaded, ids);
    }

    #[test]
    fn test_interrupted_cache_clear() {
        let dir = TempDir::new().unwrap();
        write_interrupted_cache(dir.path(), &["test_a".to_string()]);
        assert!(dir.path().join(".tach_cache/interrupted").exists());
        clear_interrupted_cache(dir.path());
        assert!(!dir.path().join(".tach_cache/interrupted").exists());
    }

    #[test]
    fn test_interrupted_cache_empty_removes_file() {
        let dir = TempDir::new().unwrap();
        write_interrupted_cache(dir.path(), &["x".to_string()]);
        write_interrupted_cache(dir.path(), &[]);
        assert!(!dir.path().join(".tach_cache/interrupted").exists());
    }
}
