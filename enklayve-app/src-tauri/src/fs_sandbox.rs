//! Sandboxed file access confined to a single user-granted working folder.
//!
//! Every path is resolved relative to the granted root and checked to ensure it
//! cannot escape via `..`, an absolute path, or a symlink. The agent may touch
//! only this folder — never the rest of the machine.

use anyhow::{bail, Context, Result};
use std::path::{Component, Path, PathBuf};

/// Resolve `requested` (relative to `root`) and ensure it stays inside `root`.
/// The target file need not exist yet (it may be newly written), but no part of
/// the resolved path may escape the root.
pub fn resolve_within(root: &Path, requested: &str) -> Result<PathBuf> {
    let root = root
        .canonicalize()
        .context("The working folder does not exist")?;

    let req = Path::new(requested);
    if req.is_absolute() {
        bail!("Absolute paths are not allowed inside the working folder");
    }

    let normalized = normalize(&root.join(req));

    if !normalized.starts_with(&root) {
        bail!("That path is outside your working folder");
    }
    // If the path (or a parent) exists as a symlink, its real location must also
    // be inside the root.
    if let Ok(canon) = normalized.canonicalize() {
        if !canon.starts_with(&root) {
            bail!("That path resolves to a location outside your working folder");
        }
    }

    Ok(normalized)
}

/// Lexically resolve `.` and `..` without touching the disk.
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Read a file from inside the working folder.
pub fn read_file(root: &Path, requested: &str) -> Result<String> {
    let path = resolve_within(root, requested)?;
    std::fs::read_to_string(&path).with_context(|| format!("Could not read {}", requested))
}

/// Write a file inside the working folder, creating parent directories as needed.
pub fn write_file(root: &Path, requested: &str, contents: &str) -> Result<PathBuf> {
    let path = resolve_within(root, requested)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&path, contents).with_context(|| format!("Could not write {}", requested))?;
    Ok(path)
}

/// List ingestible files inside the working folder, as paths relative to it.
pub fn list_files(root: &Path) -> Result<Vec<String>> {
    let files = crate::documents::collect_supported_files(root);
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    Ok(files
        .iter()
        .filter_map(|p| {
            p.strip_prefix(&root)
                .ok()
                .map(|r| r.to_string_lossy().to_string())
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "enklayve_sandbox_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    fn write_then_read_inside_sandbox() {
        let root = temp_root();
        let path = write_file(&root, "notes/out.txt", "hello").unwrap();
        assert!(path.starts_with(root.canonicalize().unwrap()));
        assert_eq!(read_file(&root, "notes/out.txt").unwrap(), "hello");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_parent_traversal() {
        let root = temp_root();
        assert!(resolve_within(&root, "../escape.txt").is_err());
        assert!(resolve_within(&root, "a/../../escape.txt").is_err());
        assert!(write_file(&root, "../evil.txt", "x").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_absolute_paths() {
        let root = temp_root();
        assert!(resolve_within(&root, "/etc/passwd").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn allows_nested_relative_paths() {
        let root = temp_root();
        let p = resolve_within(&root, "a/b/c.txt").unwrap();
        assert!(p.starts_with(root.canonicalize().unwrap()));
        let _ = std::fs::remove_dir_all(&root);
    }
}
