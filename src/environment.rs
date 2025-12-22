//! Environment detection for Tach
//!
//! Phase 8: Venv auto-detection and path injection

use std::fs;
use std::path::PathBuf;

/// Find the site-packages directory for the project's virtual environment.
///
/// Search order:
/// 1. $VIRTUAL_ENV environment variable (set by activated venvs)
/// 2. .venv directory in the project root
/// 3. venv directory in the project root
pub fn find_site_packages(project_root: &PathBuf) -> Option<PathBuf> {
    // 1. Check explicit VIRTUAL_ENV (highest priority - user explicitly activated)
    if let Ok(venv) = std::env::var("VIRTUAL_ENV") {
        let venv_path = PathBuf::from(venv);
        if let Some(sp) = find_site_packages_in_venv(&venv_path) {
            return Some(sp);
        }
    }

    // 2. Check local .venv
    let local_venv = project_root.join(".venv");
    if local_venv.exists() {
        if let Some(sp) = find_site_packages_in_venv(&local_venv) {
            return Some(sp);
        }
    }

    // 3. Check local venv (alternate naming)
    let alt_venv = project_root.join("venv");
    if alt_venv.exists() {
        if let Some(sp) = find_site_packages_in_venv(&alt_venv) {
            return Some(sp);
        }
    }

    None
}

/// Find site-packages within a virtual environment directory.
/// Linux/macOS: lib/pythonX.Y/site-packages
fn find_site_packages_in_venv(venv: &PathBuf) -> Option<PathBuf> {
    let lib = venv.join("lib");
    if !lib.exists() {
        return None;
    }

    // Look for python3.x directories
    if let Ok(entries) = fs::read_dir(&lib) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name() {
                    let name_str = name.to_string_lossy();
                    if name_str.starts_with("python") {
                        let site = path.join("site-packages");
                        if site.exists() {
                            return Some(site);
                        }
                    }
                }
            }
        }
    }

    None
}

/// Get all Python paths that should be prepended to sys.path.
/// Returns (project_root, site_packages) where site_packages may be None.
pub fn get_python_paths(project_root: &PathBuf) -> (PathBuf, Option<PathBuf>) {
    let site_packages = find_site_packages(project_root);
    (project_root.clone(), site_packages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_find_site_packages_with_venv() {
        let temp = tempdir().unwrap();
        let venv = temp.path().join(".venv");

        // Create mock venv structure
        let site_packages = venv.join("lib/python3.12/site-packages");
        fs::create_dir_all(&site_packages).unwrap();

        let project_root = temp.path().to_path_buf();
        let result = find_site_packages(&project_root);

        assert!(result.is_some());
        assert!(result.unwrap().ends_with("site-packages"));
    }

    #[test]
    fn test_find_site_packages_no_venv() {
        let temp = tempdir().unwrap();
        let project_root = temp.path().to_path_buf();

        let result = find_site_packages(&project_root);
        assert!(result.is_none());
    }

    #[test]
    fn test_virtual_env_takes_priority() {
        let temp = tempdir().unwrap();

        // Create a .venv in project root
        let local_venv = temp.path().join(".venv/lib/python3.11/site-packages");
        fs::create_dir_all(&local_venv).unwrap();

        // Create a separate venv and set VIRTUAL_ENV
        let external_venv = temp.path().join("external_venv");
        let external_site = external_venv.join("lib/python3.12/site-packages");
        fs::create_dir_all(&external_site).unwrap();

        // Set VIRTUAL_ENV
        std::env::set_var("VIRTUAL_ENV", external_venv.to_string_lossy().to_string());

        let project_root = temp.path().to_path_buf();
        let result = find_site_packages(&project_root);

        // Should find the external one (from VIRTUAL_ENV)
        assert!(result.is_some());
        let result_path = result.unwrap();
        assert!(result_path.to_string_lossy().contains("external_venv"));

        // Cleanup
        std::env::remove_var("VIRTUAL_ENV");
    }
}
