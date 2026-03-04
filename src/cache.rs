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
