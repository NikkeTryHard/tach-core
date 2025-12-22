//! Watch Mode: Automatic test re-execution on file changes
//!
//! Phase 5.3: The Feedback Loop
//!
//! ## Critical: Stale Zygote Problem
//!
//! Workers fork from Zygote which has old code in memory.
//! Changed files on disk won't be seen unless we recycle the Zygote.
//! This module respawns the entire test session on each change.

use anyhow::Result;
use crossbeam_channel::{unbounded, Receiver};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Clear terminal screen (ANSI escape codes)
pub fn clear_screen() {
    print!("\x1B[2J\x1B[1;1H");
    use std::io::Write;
    let _ = std::io::stdout().flush();
}

/// Start the watch loop - blocks forever until Ctrl+C
///
/// # Arguments
/// * `project_root` - Directory to watch for changes
/// * `run_session` - Callback to execute a full test session
///
pub fn start_watch_loop<F>(project_root: &Path, mut run_session: F) -> Result<()>
where
    F: FnMut() -> Result<()>,
{
    let (tx, rx) = unbounded();

    // Create watcher with default config
    let mut watcher = RecommendedWatcher::new(
        move |res: notify::Result<Event>| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        },
        Config::default(),
    )?;

    // Watch the project directory recursively
    watcher.watch(project_root, RecursiveMode::Recursive)?;

    eprintln!(
        "[tach] 👁  Watching for changes in {}",
        project_root.display()
    );
    eprintln!("[tach] Press Ctrl+C to stop.\n");

    // Initial run
    if let Err(e) = run_session() {
        eprintln!("[tach] Initial run failed: {}", e);
    }

    // Event loop
    loop {
        // Wait for first event
        match rx.recv() {
            Ok(first_event) => {
                // Collect affected paths
                let mut changed_paths = collect_python_paths(&first_event);

                // Debounce: accumulate events until 100ms of silence
                while let Ok(event) = rx.recv_timeout(Duration::from_millis(100)) {
                    changed_paths.extend(collect_python_paths(&event));
                }

                // Filter: only .py file changes trigger re-run
                if changed_paths.is_empty() {
                    continue;
                }

                // === CRITICAL: Full Session Recycle ===
                // This respawns the Zygote to pick up new source code
                clear_screen();
                eprintln!(
                    "[tach] 🔄 Change detected in {} file(s). Reloading...\n",
                    changed_paths.len()
                );

                if let Err(e) = run_session() {
                    eprintln!("[tach] Run failed: {}", e);
                }
            }
            Err(_) => {
                // Channel closed - watcher dropped
                break;
            }
        }
    }

    Ok(())
}

/// Extract Python file paths from a notify event
fn collect_python_paths(event: &Event) -> Vec<PathBuf> {
    event
        .paths
        .iter()
        .filter(|p| p.extension() == Some(OsStr::new("py")))
        .filter(|p| !is_ignored_path(p))
        .cloned()
        .collect()
}

/// Check if a path should be ignored
fn is_ignored_path(path: &Path) -> bool {
    let path_str = path.to_string_lossy();

    // Ignore common patterns
    path_str.contains("__pycache__")
        || path_str.contains(".pytest_cache")
        || path_str.contains(".mypy_cache")
        || path_str.contains(".git")
        || path_str.contains(".venv")
        || path_str.contains("/venv/")
        || path_str.contains("/env/")
        || path_str.contains("/node_modules/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_ignored_path() {
        assert!(is_ignored_path(Path::new("foo/__pycache__/bar.py")));
        assert!(is_ignored_path(Path::new(".git/hooks/pre-commit.py")));
        assert!(is_ignored_path(Path::new(".venv/lib/python3.10/site.py")));
        assert!(!is_ignored_path(Path::new("tests/test_foo.py")));
        assert!(!is_ignored_path(Path::new("src/models.py")));
    }
}
