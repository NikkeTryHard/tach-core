use serde::{Deserialize, Serialize};
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

const MAX_HISTORY_RUNS: usize = 20;

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct TestHistory {
    pub runs: Vec<RunRecord>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RunRecord {
    pub timestamp: u64,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub duration_ms: u64,
    pub test_durations: HashMap<String, u64>,
    pub failed_tests: Vec<String>,
}

impl TestHistory {
    pub fn load(root: &Path) -> Self {
        let path = root.join(".tach_cache/history.json");
        match std::fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, root: &Path) {
        let cache_dir = root.join(".tach_cache");
        let _ = std::fs::create_dir_all(&cache_dir);
        let path = cache_dir.join("history.json");
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, json);
        }
    }

    pub fn add_run(&mut self, record: RunRecord) {
        self.runs.push(record);
        if self.runs.len() > MAX_HISTORY_RUNS {
            self.runs.drain(0..self.runs.len() - MAX_HISTORY_RUNS);
        }
    }

    pub fn avg_duration(&self, test_name: &str) -> Option<u64> {
        let mut total = 0u64;
        let mut count = 0u64;
        for run in &self.runs {
            if let Some(&ms) = run.test_durations.get(test_name) {
                total += ms;
                count += 1;
            }
        }
        if count > 0 { Some(total / count) } else { None }
    }

    pub fn flaky_tests(&self) -> Vec<String> {
        let mut results: HashMap<&str, (usize, usize)> = HashMap::new();
        for run in &self.runs {
            for name in run.test_durations.keys() {
                let entry = results.entry(name.as_str()).or_default();
                if run.failed_tests.contains(name) {
                    entry.1 += 1;
                } else {
                    entry.0 += 1;
                }
            }
        }
        results
            .into_iter()
            .filter(|(_, (pass, fail))| *pass > 0 && *fail > 0)
            .map(|(name, _)| name.to_string())
            .collect()
    }

    pub fn pass_rate(&self) -> f64 {
        if self.runs.is_empty() {
            return 0.0;
        }
        let total_passed: usize = self.runs.iter().map(|r| r.passed).sum();
        let total_tests: usize = self.runs.iter().map(|r| r.total).sum();
        if total_tests == 0 {
            return 0.0;
        }
        total_passed as f64 / total_tests as f64 * 100.0
    }
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

    #[test]
    fn test_interrupted_cache_preserves_order() {
        let dir = TempDir::new().unwrap();
        let ids: Vec<String> = (0..100).map(|i| format!("test_{i}")).collect();
        write_interrupted_cache(dir.path(), &ids);
        let loaded = read_interrupted_cache(dir.path());
        assert_eq!(loaded, ids);
    }

    #[test]
    fn test_interrupted_cache_with_special_chars() {
        let dir = TempDir::new().unwrap();
        let ids = vec![
            "test_file.py::TestClass::test_method[param1]".to_string(),
            "tests/sub dir/test_foo.py::test_bar".to_string(),
        ];
        write_interrupted_cache(dir.path(), &ids);
        let loaded = read_interrupted_cache(dir.path());
        assert_eq!(loaded, ids);
    }

    #[test]
    fn test_clear_nonexistent_interrupted_cache() {
        let dir = TempDir::new().unwrap();
        clear_interrupted_cache(dir.path());
    }

    #[test]
    fn test_read_interrupted_cache_nonexistent() {
        let dir = TempDir::new().unwrap();
        let loaded = read_interrupted_cache(dir.path());
        assert!(loaded.is_empty());
    }

    fn make_run(passed: usize, failed: usize, durations: &[(&str, u64)]) -> RunRecord {
        RunRecord {
            timestamp: 1000,
            total: passed + failed,
            passed,
            failed,
            skipped: 0,
            duration_ms: durations.iter().map(|(_, d)| d).sum(),
            test_durations: durations.iter().map(|(n, d)| (n.to_string(), *d)).collect(),
            failed_tests: vec![],
        }
    }

    #[test]
    fn test_history_save_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let mut history = TestHistory::default();
        history.add_run(make_run(10, 2, &[("test_a", 100), ("test_b", 200)]));
        history.save(dir.path());

        let loaded = TestHistory::load(dir.path());
        assert_eq!(loaded.runs.len(), 1);
        assert_eq!(loaded.runs[0].passed, 10);
        assert_eq!(loaded.runs[0].failed, 2);
    }

    #[test]
    fn test_history_max_runs_capped() {
        let mut history = TestHistory::default();
        for i in 0..30 {
            history.add_run(RunRecord {
                timestamp: i,
                total: 1,
                passed: 1,
                failed: 0,
                skipped: 0,
                duration_ms: 100,
                test_durations: HashMap::new(),
                failed_tests: vec![],
            });
        }
        assert_eq!(history.runs.len(), MAX_HISTORY_RUNS);
    }

    #[test]
    fn test_history_avg_duration() {
        let mut history = TestHistory::default();
        history.add_run(make_run(1, 0, &[("test_a", 100)]));
        history.add_run(make_run(1, 0, &[("test_a", 200)]));
        history.add_run(make_run(1, 0, &[("test_a", 300)]));
        assert_eq!(history.avg_duration("test_a"), Some(200));
        assert_eq!(history.avg_duration("nonexistent"), None);
    }

    #[test]
    fn test_history_flaky_detection() {
        let mut history = TestHistory::default();
        let mut run1 = make_run(2, 0, &[("test_a", 100), ("test_b", 50)]);
        run1.failed_tests = vec![];
        history.add_run(run1);

        let mut run2 = make_run(1, 1, &[("test_a", 100), ("test_b", 50)]);
        run2.failed_tests = vec!["test_a".to_string()];
        history.add_run(run2);

        let flaky = history.flaky_tests();
        assert!(flaky.contains(&"test_a".to_string()));
        assert!(!flaky.contains(&"test_b".to_string()));
    }

    #[test]
    fn test_history_pass_rate() {
        let mut history = TestHistory::default();
        history.add_run(make_run(8, 2, &[]));
        history.add_run(make_run(9, 1, &[]));
        let rate = history.pass_rate();
        assert!((rate - 85.0).abs() < 0.1);
    }

    #[test]
    fn test_history_empty_pass_rate() {
        let history = TestHistory::default();
        assert!((history.pass_rate() - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_history_load_nonexistent() {
        let dir = TempDir::new().unwrap();
        let history = TestHistory::load(dir.path());
        assert!(history.runs.is_empty());
    }
}
