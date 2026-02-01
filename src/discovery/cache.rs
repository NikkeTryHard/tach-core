//! Disk-based cache for conftest.py parsing results.
//!
//! This module provides persistent caching of conftest.py analysis to avoid
//! re-parsing on every test run. The cache stores fixture definitions, hook
//! definitions, and metadata about each conftest file.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

// =============================================================================
// Cache Entry
// =============================================================================

/// Cached information about a single conftest.py file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConftestCacheEntry {
    /// Absolute path to the conftest.py file
    pub path: PathBuf,

    /// Modification time when the file was cached
    pub mtime: SystemTime,

    /// SHA-256 hash of file contents for validation
    pub content_hash: String,

    /// Names of fixtures defined in this conftest
    pub fixture_names: Vec<String>,

    /// Names of pytest hooks defined in this conftest
    pub hook_names: Vec<String>,

    /// Whether this conftest has any autouse fixtures
    pub has_autouse: bool,
}

impl ConftestCacheEntry {
    /// Create a new cache entry.
    pub fn new(
        path: PathBuf,
        mtime: SystemTime,
        content_hash: String,
        fixture_names: Vec<String>,
        hook_names: Vec<String>,
        has_autouse: bool,
    ) -> Self {
        Self {
            path,
            mtime,
            content_hash,
            fixture_names,
            hook_names,
            has_autouse,
        }
    }

    /// Check if this cache entry is still valid for the given file.
    ///
    /// Returns true if the file exists and has the same mtime.
    pub fn is_valid(&self) -> bool {
        if let Ok(metadata) = fs::metadata(&self.path)
            && let Ok(current_mtime) = metadata.modified()
        {
            return current_mtime == self.mtime;
        }
        false
    }

    /// Check if the content hash matches the current file contents.
    ///
    /// This is a more expensive but more reliable validation than mtime alone.
    pub fn validate_hash(&self) -> bool {
        if let Ok(contents) = fs::read_to_string(&self.path) {
            let hash = compute_hash(&contents);
            return hash == self.content_hash;
        }
        false
    }
}

// =============================================================================
// Cache Statistics
// =============================================================================

/// Statistics about cache usage.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Number of entries in the cache
    pub entries: usize,

    /// Number of cache hits
    pub hits: usize,

    /// Number of cache misses
    pub misses: usize,

    /// Number of invalidated entries (stale mtime)
    pub invalidated: usize,
}

impl CacheStats {
    /// Calculate the hit rate as a percentage.
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            (self.hits as f64 / total as f64) * 100.0
        }
    }
}

// =============================================================================
// Cache Implementation
// =============================================================================

/// Disk-based cache for conftest.py parsing results.
///
/// The cache is stored as a JSON file in the project's `.tach` directory.
/// Cache entries are keyed by the absolute path of the conftest.py file.
#[derive(Debug)]
pub struct ConftestCache {
    /// Path to the cache file
    cache_path: PathBuf,

    /// In-memory cache entries, keyed by absolute path
    entries: HashMap<PathBuf, ConftestCacheEntry>,

    /// Cache statistics
    stats: CacheStats,
}

impl ConftestCache {
    /// Create a new cache for the given project root.
    ///
    /// This will load any existing cache from disk, or create a new empty cache.
    pub fn new(project_root: &Path) -> io::Result<Self> {
        let cache_dir = project_root.join(".tach");
        let cache_path = cache_dir.join("conftest_cache.json");

        // Ensure cache directory exists
        if !cache_dir.exists() {
            fs::create_dir_all(&cache_dir)?;
        }

        // Load existing cache or start fresh
        let entries = if cache_path.exists() {
            match fs::read_to_string(&cache_path) {
                Ok(contents) => serde_json::from_str(&contents).unwrap_or_else(|_| HashMap::new()),
                Err(_) => HashMap::new(),
            }
        } else {
            HashMap::new()
        };

        let entry_count = entries.len();

        Ok(Self {
            cache_path,
            entries,
            stats: CacheStats {
                entries: entry_count,
                ..Default::default()
            },
        })
    }

    /// Get a cached entry for the given conftest path.
    ///
    /// Returns None if the entry doesn't exist or is stale (mtime changed).
    pub fn get(&mut self, path: &Path) -> Option<&ConftestCacheEntry> {
        // Canonicalize path for consistent lookup
        let canonical = path.canonicalize().ok()?;

        if let Some(entry) = self.entries.get(&canonical) {
            if entry.is_valid() {
                self.stats.hits += 1;
                // Return the entry - we need to re-borrow to satisfy borrow checker
                return self.entries.get(&canonical);
            } else {
                self.stats.invalidated += 1;
            }
        }

        self.stats.misses += 1;
        None
    }

    /// Insert or update a cache entry.
    pub fn insert(&mut self, entry: ConftestCacheEntry) {
        // Canonicalize path for consistent storage
        let canonical = entry
            .path
            .canonicalize()
            .unwrap_or_else(|_| entry.path.clone());
        self.entries.insert(canonical, entry);
        self.stats.entries = self.entries.len();
    }

    /// Save the cache to disk.
    pub fn save(&self) -> io::Result<()> {
        let contents = serde_json::to_string_pretty(&self.entries)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(&self.cache_path, contents)
    }

    /// Clear all cache entries and remove the cache file.
    pub fn clear(&mut self) -> io::Result<()> {
        self.entries.clear();
        self.stats = CacheStats::default();

        if self.cache_path.exists() {
            fs::remove_file(&self.cache_path)?;
        }

        Ok(())
    }

    /// Get cache statistics.
    pub fn stats(&self) -> CacheStats {
        self.stats.clone()
    }

    /// Get the number of cached entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Remove stale entries from the cache.
    ///
    /// Returns the number of entries removed.
    pub fn prune_stale(&mut self) -> usize {
        let before = self.entries.len();
        self.entries.retain(|_, entry| entry.is_valid());
        let removed = before - self.entries.len();
        self.stats.entries = self.entries.len();
        removed
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Compute a simple hash of file contents.
///
/// Uses a basic hash for speed - not cryptographically secure but sufficient
/// for cache invalidation purposes.
pub fn compute_hash(contents: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    contents.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_conftest(dir: &Path, content: &str) -> PathBuf {
        let path = dir.join("conftest.py");
        let mut file = File::create(&path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn test_cache_new_creates_directory() {
        let temp = TempDir::new().unwrap();
        let cache = ConftestCache::new(temp.path()).unwrap();

        assert!(temp.path().join(".tach").exists());
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_insert_and_get() {
        let temp = TempDir::new().unwrap();
        let conftest_path = create_conftest(temp.path(), "import pytest\n");

        let mut cache = ConftestCache::new(temp.path()).unwrap();

        let entry = ConftestCacheEntry::new(
            conftest_path.clone(),
            fs::metadata(&conftest_path).unwrap().modified().unwrap(),
            compute_hash("import pytest\n"),
            vec!["my_fixture".to_string()],
            vec!["pytest_configure".to_string()],
            false,
        );

        cache.insert(entry);

        let retrieved = cache.get(&conftest_path);
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.fixture_names, vec!["my_fixture"]);
        assert_eq!(retrieved.hook_names, vec!["pytest_configure"]);
        assert!(!retrieved.has_autouse);
    }

    #[test]
    fn test_cache_save_and_load() {
        let temp = TempDir::new().unwrap();
        let conftest_path = create_conftest(temp.path(), "# test\n");

        // Create and populate cache
        {
            let mut cache = ConftestCache::new(temp.path()).unwrap();
            let entry = ConftestCacheEntry::new(
                conftest_path.clone(),
                fs::metadata(&conftest_path).unwrap().modified().unwrap(),
                compute_hash("# test\n"),
                vec!["fixture_a".to_string(), "fixture_b".to_string()],
                vec![],
                true,
            );
            cache.insert(entry);
            cache.save().unwrap();
        }

        // Load cache in new instance
        {
            let mut cache = ConftestCache::new(temp.path()).unwrap();
            assert_eq!(cache.len(), 1);

            let retrieved = cache.get(&conftest_path).unwrap();
            assert_eq!(retrieved.fixture_names, vec!["fixture_a", "fixture_b"]);
            assert!(retrieved.has_autouse);
        }
    }

    #[test]
    fn test_cache_invalidation_on_mtime_change() {
        let temp = TempDir::new().unwrap();
        let conftest_path = create_conftest(temp.path(), "# original\n");

        let mut cache = ConftestCache::new(temp.path()).unwrap();

        let entry = ConftestCacheEntry::new(
            conftest_path.clone(),
            fs::metadata(&conftest_path).unwrap().modified().unwrap(),
            compute_hash("# original\n"),
            vec!["old_fixture".to_string()],
            vec![],
            false,
        );
        cache.insert(entry);

        // Verify entry exists
        assert!(cache.get(&conftest_path).is_some());

        // Modify the file (changes mtime)
        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(&conftest_path, "# modified\n").unwrap();

        // Entry should now be invalid
        assert!(cache.get(&conftest_path).is_none());
        assert_eq!(cache.stats().invalidated, 1);
    }

    #[test]
    fn test_cache_clear() {
        let temp = TempDir::new().unwrap();
        let conftest_path = create_conftest(temp.path(), "# test\n");

        let mut cache = ConftestCache::new(temp.path()).unwrap();
        let entry = ConftestCacheEntry::new(
            conftest_path,
            SystemTime::now(),
            "hash".to_string(),
            vec![],
            vec![],
            false,
        );
        cache.insert(entry);
        cache.save().unwrap();

        assert_eq!(cache.len(), 1);
        assert!(temp.path().join(".tach/conftest_cache.json").exists());

        cache.clear().unwrap();

        assert!(cache.is_empty());
        assert!(!temp.path().join(".tach/conftest_cache.json").exists());
    }

    #[test]
    fn test_cache_stats() {
        let temp = TempDir::new().unwrap();
        let conftest_path = create_conftest(temp.path(), "# test\n");

        let mut cache = ConftestCache::new(temp.path()).unwrap();

        // Miss on empty cache
        assert!(cache.get(&conftest_path).is_none());
        assert_eq!(cache.stats().misses, 1);
        assert_eq!(cache.stats().hits, 0);

        // Insert entry
        let entry = ConftestCacheEntry::new(
            conftest_path.clone(),
            fs::metadata(&conftest_path).unwrap().modified().unwrap(),
            compute_hash("# test\n"),
            vec![],
            vec![],
            false,
        );
        cache.insert(entry);

        // Hit on populated cache
        assert!(cache.get(&conftest_path).is_some());
        assert_eq!(cache.stats().hits, 1);
        assert_eq!(cache.stats().misses, 1);

        // Hit rate should be 50%
        assert!((cache.stats().hit_rate() - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_cache_prune_stale() {
        let temp = TempDir::new().unwrap();
        let conftest_path = create_conftest(temp.path(), "# test\n");

        let mut cache = ConftestCache::new(temp.path()).unwrap();

        // Add entry for existing file
        let entry = ConftestCacheEntry::new(
            conftest_path.clone(),
            fs::metadata(&conftest_path).unwrap().modified().unwrap(),
            compute_hash("# test\n"),
            vec![],
            vec![],
            false,
        );
        cache.insert(entry);

        // Add entry for non-existent file
        let fake_entry = ConftestCacheEntry::new(
            PathBuf::from("/nonexistent/conftest.py"),
            SystemTime::now(),
            "fake".to_string(),
            vec![],
            vec![],
            false,
        );
        cache.insert(fake_entry);

        assert_eq!(cache.len(), 2);

        // Prune should remove the non-existent file entry
        let removed = cache.prune_stale();
        assert_eq!(removed, 1);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_compute_hash_consistency() {
        let content = "import pytest\n\n@pytest.fixture\ndef my_fixture():\n    pass\n";
        let hash1 = compute_hash(content);
        let hash2 = compute_hash(content);
        assert_eq!(hash1, hash2);

        let different = "different content";
        let hash3 = compute_hash(different);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_entry_validate_hash() {
        let temp = TempDir::new().unwrap();
        let content = "# hash test\n";
        let conftest_path = create_conftest(temp.path(), content);

        let entry = ConftestCacheEntry::new(
            conftest_path.clone(),
            SystemTime::now(),
            compute_hash(content),
            vec![],
            vec![],
            false,
        );

        assert!(entry.validate_hash());

        // Modify file content
        fs::write(&conftest_path, "# different content\n").unwrap();
        assert!(!entry.validate_hash());
    }
}
