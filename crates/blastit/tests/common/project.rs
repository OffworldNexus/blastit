//! A temporary project root, shared by every `blastit dev` fixture.

use std::path::{Path, PathBuf};

/// A temporary project, removed when dropped.
pub struct Project {
    /// The directory `blastit dev` is started in.
    pub dir: PathBuf,
}

impl Project {
    /// A fresh directory for one case, with no `Blastfile` yet.
    pub fn new(tag: &str) -> Self {
        let dir = scratch_root().join(format!("blastit-e2e-dev-{}-{tag}", std::process::id()));
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

/// `/tmp/opencode` when it exists, the platform's temporary directory otherwise.
fn scratch_root() -> PathBuf {
    let preferred = Path::new("/tmp/opencode");
    if std::fs::create_dir_all(preferred).is_ok() {
        return preferred.to_path_buf();
    }
    std::env::temp_dir()
}
