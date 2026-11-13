//! Recursive directory size calculator.
//!
//! Walks a directory tree and returns total byte count and file count. Symlinks are
//! followed by default, but a set of visited canonical paths prevents infinite cycles.
//!
//! The library exposes a single public function [`calculate`] that returns a
//! [`DirectoryStats`] struct.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Summary statistics for a directory tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryStats {
    /// Total size in bytes of all regular files encountered.
    pub total_bytes: u64,
    /// Number of regular files encountered.
    pub file_count: u64,
}

/// Recursively computes the total size and file count of `root`.
///
/// Symlinks are followed. To avoid infinite recursion in the presence of symlink
/// cycles, each canonical directory path is visited at most once. If a canonical
/// path cannot be resolved (e.g. broken symlink), the entry is skipped.
///
/// # Errors
///
/// Returns an error if `root` cannot be read as a directory. Errors encountered
/// while traversing individual entries are silently ignored so that a single
/// unreadable file does not abort the whole walk.
pub fn calculate<P: AsRef<Path>>(root: P) -> io::Result<DirectoryStats> {
    let root = root.as_ref();
    let metadata = fs::metadata(root)?;
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "root path is not a directory",
        ));
    }

    let mut stats = DirectoryStats {
        total_bytes: 0,
        file_count: 0,
    };
    let mut visited = HashSet::new();
    walk(root, &mut visited, &mut stats);
    Ok(stats)
}

fn walk(dir: &Path, visited: &mut HashSet<PathBuf>, stats: &mut DirectoryStats) {
    // Canonicalize the directory so that different symlink paths to the same
    // directory are only traversed once. This is the cycle guard.
    let canonical = match fs::canonicalize(dir) {
        Ok(p) => p,
        Err(_) => return,
    };
    if !visited.insert(canonical) {
        return;
    }

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        // Use symlink_metadata to detect symlinks without following them yet.
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        if meta.file_type().is_symlink() {
            // Follow the symlink to see what it points to.
            match fs::metadata(&path) {
                Ok(target_meta) => {
                    if target_meta.is_dir() {
                        walk(&path, visited, stats);
                    } else if target_meta.is_file() {
                        stats.total_bytes += target_meta.len();
                        stats.file_count += 1;
                    }
                }
                Err(_) => continue, // broken symlink
            }
        } else if meta.is_dir() {
            walk(&path, visited, stats);
        } else if meta.is_file() {
            stats.total_bytes += meta.len();
            stats.file_count += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use std::path::PathBuf;

    /// Creates a unique temporary directory for a test.
    fn temp_dir(name: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "recursive-directory-size-calculator-test-{}-{}",
            name,
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn write_file(path: &Path, contents: &[u8]) {
        let mut f = File::create(path).expect("create file");
        f.write_all(contents).expect("write file");
    }

    #[test]
    fn empty_directory() {
        let dir = temp_dir("empty");
        let stats = calculate(&dir).expect("calculate");
        assert_eq!(stats.total_bytes, 0);
        assert_eq!(stats.file_count, 0);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn single_file() {
        let dir = temp_dir("single");
        write_file(&dir.join("a.txt"), b"hello");
        let stats = calculate(&dir).expect("calculate");
        assert_eq!(stats.total_bytes, 5);
        assert_eq!(stats.file_count, 1);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn nested_files() {
        let dir = temp_dir("nested");
        fs::create_dir(dir.join("sub")).unwrap();
        write_file(&dir.join("a.txt"), b"abc");
        write_file(&dir.join("sub").join("b.txt"), b"defg");
        let stats = calculate(&dir).expect("calculate");
        assert_eq!(stats.total_bytes, 7);
        assert_eq!(stats.file_count, 2);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn symlink_to_file() {
        let dir = temp_dir("symlink-file");
        write_file(&dir.join("target.txt"), b"data");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.join("target.txt"), dir.join("link.txt")).unwrap();
        }
        #[cfg(windows)]
        {
            std::os::windows::fs::symlink_file(dir.join("target.txt"), dir.join("link.txt"))
                .unwrap();
        }
        let stats = calculate(&dir).expect("calculate");
        // Both the target and the link are counted as files.
        assert_eq!(stats.total_bytes, 8);
        assert_eq!(stats.file_count, 2);
        fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn symlink_cycle_does_not_hang() {
        let dir = temp_dir("cycle");
        let sub = dir.join("sub");
        fs::create_dir(&sub).unwrap();
        write_file(&sub.join("f.txt"), b"x");
        // Create a symlink inside sub that points back to dir.
        std::os::unix::fs::symlink(&dir, sub.join("back")).unwrap();
        let stats = calculate(&dir).expect("calculate");
        assert_eq!(stats.file_count, 1);
        assert_eq!(stats.total_bytes, 1);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn broken_symlink_is_ignored() {
        let dir = temp_dir("broken-symlink");
        write_file(&dir.join("real.txt"), b"real");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.join("missing.txt"), dir.join("dangling"))
                .unwrap();
        }
        #[cfg(windows)]
        {
            std::os::windows::fs::symlink_file(dir.join("missing.txt"), dir.join("dangling"))
                .unwrap();
        }
        let stats = calculate(&dir).expect("calculate");
        assert_eq!(stats.total_bytes, 4);
        assert_eq!(stats.file_count, 1);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn non_directory_root_errors() {
        let dir = temp_dir("not-dir");
        let file = dir.join("file.txt");
        write_file(&file, b"nope");
        let err = calculate(&file).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        fs::remove_dir_all(&dir).ok();
    }
}
