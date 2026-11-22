//! A temporary project root, shared by every `blastit dev` fixture.

use std::path::PathBuf;

/// A temporary project, removed when dropped.
pub struct Project {
    /// The directory `blastit dev` is started in.
    pub dir: PathBuf,
}

impl Project {
    /// A fresh directory for one case, with no `Blastfile` yet.
    pub fn new(tag: &str) -> Self {
        let dir = scratch_root().join(tag);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the fixture directory");
        Self { dir }
    }

    /// The absolute path of `relative` inside the project.
    pub fn path(&self, relative: &str) -> PathBuf {
        self.dir.join(relative)
    }

    /// Writes `contents` to `relative`, creating its parent directories.
    pub fn write(&self, relative: &str, contents: impl AsRef<[u8]>) -> PathBuf {
        let path = self.path(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("a fixture parent directory");
        }
        std::fs::write(&path, contents).expect("a fixture file");
        path
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The process temporary directory (`TMPDIR` aware) with one short per-run segment.
///
/// The root must stay short: a fixture binds its PostgreSQL socket at
/// `<root>/<tag>/tmp/blastit-check-<uid>-<pid>-<rand>-0/pg/.s.PGSQL.5432`, and a
/// `sun_path` has to stay under `SUN_LEN` (108 bytes, including the trailing NUL).
/// `/tmp/` plus that suffix alone is ~61 bytes, so a fixed multi-segment root such
/// as `/tmp/opencode` leaves no budget once the pid and test tag are appended.
fn scratch_root() -> PathBuf {
    let root = std::env::temp_dir().join(format!("blastit-e2e-{}", std::process::id()));
    let len = root.to_string_lossy().len();
    assert!(
        len <= 48,
        "fixture root `{}` is {len} bytes; the deepest socket adds \
         `blastit-check-<uid>-<pid>-<rand>-0/pg/.s.PGSQL.5432` and must stay under SUN_LEN (108)",
        root.display()
    );
    root
}
