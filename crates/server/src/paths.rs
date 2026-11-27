use std::env;
use std::path::{Component, Path, PathBuf};

/// Node's `path.resolve`: absolute and free of `.` and `..`, without touching the
/// disk. Stored paths were built this way, so canonicalising here would stop a
/// symlinked library matching its own database rows.
pub fn resolve(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(path)
    };

    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            other => resolved.push(other),
        }
    }
    resolved
}

/// True when `child` resolves to something strictly inside `parent`.
///
/// File paths reaching the stream and artwork handlers come from the database
/// rather than the URL, but a bad scan or a future caller must not be able to
/// turn this into an arbitrary file reader.
pub fn is_inside(parent: &Path, child: &Path) -> bool {
    let parent = resolve(parent);
    let child = resolve(child);
    child != parent && child.starts_with(parent)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inside(parent: &str, child: &str) -> bool {
        is_inside(parent.as_ref(), child.as_ref())
    }

    #[test]
    fn accepts_a_file_within_the_library() {
        assert!(inside("/music", "/music/artist/album/song.mp3"));
    }

    #[test]
    fn rejects_traversal_out_of_the_library() {
        assert!(!inside("/music", "/music/../etc/passwd"));
        assert!(!inside("/music", "/etc/passwd"));
    }

    #[test]
    fn rejects_a_sibling_with_a_shared_prefix() {
        assert!(!inside("/music", "/music-private/secret.mp3"));
    }

    #[test]
    fn rejects_the_library_root_itself() {
        assert!(!inside("/music", "/music"));
    }
}
