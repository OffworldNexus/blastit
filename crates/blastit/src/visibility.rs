//! Whether the uids that serve requests can read what a view imports (§2.1,
//! §5.3, §7.2).
//!
//! A view process runs as a uid of its own, and it is not the owner's: the
//! configuration, the interpreter and the application's code belong to whoever
//! installed them, so the only permission bits that apply inside a view are
//! **other**'s (`o`). A directory the owner can read and traverse is not
//! necessarily one a view can, and the failure it produces is in the middle of
//! a request: the modules an application imports *lazily* — `stringprep` and
//! `encodings.idna` behind Django's URL parsing, a template, a URLconf that is
//! not in `BLASTIT_PRELOAD` — are imported after the fork, from a directory
//! nobody has looked at since the server started.
//!
//! The failure mode is nasty enough to be worth a refusal at boot instead:
//! `libpython`'s built-ins keep working, so the app starts, answers the health
//! probe and serves some requests, and then a view raises
//! `ModuleNotFoundError: No module named 'stringprep'` on one path and nothing
//! else. The cost of refusing is a deployment that has to move an interpreter
//! somewhere every uid can reach; the cost of not refusing is a production
//! incident whose message says nothing about permissions.
//!
//! # What is checked
//!
//! Every path a view imports *through*: the interpreter's prefixes, the
//! standard library, `site-packages`, and the directory the application's own
//! module comes from. For each of them, every directory on the way down must be
//! traversable by others (`o+x`) and the directory itself readable (`o+r`) —
//! reading a module means descending into the directory that holds it. The
//! first directory that fails is the one the message names, because it is the
//! one an operator has to change.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// What a view cannot do with a directory, in the words of the message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// Read it: list it, and read the files in it.
    Read,
    /// Traverse it: reach what is below it.
    Traverse,
}

impl Access {
    /// The words [`Blocker::message`] uses for this access.
    pub const fn phrase(self) -> &'static str {
        match self {
            Self::Read => "not readable by other uids",
            Self::Traverse => "not traversable by other uids",
        }
    }
}

/// A directory a view uid cannot reach, and what is wrong with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blocker {
    /// The directory that blocks, as it was asked for.
    pub path: PathBuf,
    /// Its permission bits, exactly as `chmod` writes them.
    pub mode: u32,
    /// What a view cannot do with it.
    pub access: Access,
}

impl Blocker {
    /// The sentence an operator reads: what is wrong, and what to do about it.
    ///
    /// It names the directory, its mode, and both fixes: moving what is
    /// unreachable somewhere every uid can read — for an interpreter, a
    /// system-wide Python, or `UV_PYTHON_INSTALL_DIR` for a uv-managed one —
    /// or opening the way to it, which is the whole path rather than the one
    /// directory, so the message says that too.
    pub fn message(&self) -> String {
        format!(
            "{} (mode {:04o}) is {}: a view process runs as a uid of its own (§2.1), so \
             only the \"other\" permission bits apply to it, and a view that cannot reach \
             this directory loses the modules it imports lazily (`ModuleNotFoundError: \
             No module named 'stringprep'`, `encodings.idna`, …). Two fixes: move it \
             somewhere every uid can read — for an interpreter, a system-wide Python, or \
             `export UV_PYTHON_INSTALL_DIR=/opt/uv-python` before `uv python install` — \
             or open the way to it: `chmod o+rx {}` here and on every directory above it",
            self.path.display(),
            self.mode,
            self.access.phrase(),
            self.path.display(),
        )
    }
}

/// The first directory of `paths` a view uid could not read or traverse.
///
/// The paths are what a view imports through (see the module documentation), in
/// the order an operator should read them: the interpreter first, because that
/// is where a managed Python under a private home blocks, then the application's
/// own code.
///
/// A path that is not there, or is not a directory, is not this check's
/// business: an import that needs a missing directory fails with its own
/// message, and claiming a permission problem there would send an operator to
/// `chmod` something that does not exist.
pub fn unreadable(paths: &[PathBuf]) -> Option<Blocker> {
    paths.iter().find_map(|path| blocks(path))
}

/// The first directory on the way to `path` that a view uid could not reach.
///
/// The directory itself must be readable *and* traversable — a module inside it
/// is reached by descending into it — and every directory above it must be
/// traversable. Symlinks are followed, because the kernel resolves them: the
/// directory a view has to be allowed through is the target's, not the link's.
fn blocks(path: &Path) -> Option<Blocker> {
    for (depth, directory) in path.ancestors().enumerate() {
        if directory.as_os_str().is_empty() {
            continue;
        }
        let Ok(metadata) = std::fs::metadata(directory) else {
            // The directory is not there (or not readable to us, which as root
            // or init's uid means it is not there): whatever needed it fails
            // with its own message.
            continue;
        };
        if !metadata.is_dir() {
            continue;
        }
        let mode = metadata.permissions().mode() & 0o777;
        let access = match (mode & 0o004 == 0, mode & 0o001 == 0) {
            // The directory a file is in is both read and descended into; a
            // directory on the way to it is only descended into.
            (true, _) if depth == 0 => Some(Access::Read),
            (_, true) => Some(Access::Traverse),
            _ => None,
        };
        if let Some(access) = access {
            return Some(Blocker {
                path: directory.to_path_buf(),
                mode,
                access,
            });
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    /// A directory tree under `/tmp`, removed when it is dropped.
    ///
    /// `/tmp` rather than `$HOME`, and deliberately: these tests are about the
    /// bits another uid needs, and every directory *above* the tree is part of
    /// the answer. `/tmp` is `1777` and `/` is `0755` on every machine blastit
    /// runs on, so the first blocker a test finds is one it created itself —
    /// where a `$HOME` tree would report the real home above it (`0750` in the
    /// very layout this module exists for).
    struct TempTree {
        path: PathBuf,
    }

    impl TempTree {
        fn new(tag: &str) -> Self {
            let path = PathBuf::from("/tmp")
                .join(format!(".blastit-visibility-{}-{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            Self { path }
        }

        /// A directory inside the tree, with `mode`.
        fn dir(&self, name: &str, mode: u32) -> PathBuf {
            let path = self.path.join(name);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            path
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn a_directory_every_one_can_read_is_not_a_blocker() {
        let tree = TempTree::new("open");
        let open = tree.dir("lib", 0o755);
        let inside = tree.dir("lib/python3.14", 0o755);
        assert_eq!(unreadable(&[open.clone(), inside]), None);
        // A directory that is not there is not a permission problem.
        assert_eq!(unreadable(&[tree.path.join("missing")]), None);
        // Neither is a file: `sys.prefix` is a directory, but a path that is
        // not one has no mode to report.
        let file = tree.path.join("os.py");
        std::fs::write(&file, b"").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(unreadable(&[file]), None);
    }

    #[test]
    fn a_leaf_a_view_could_not_read_names_itself_and_its_mode() {
        let tree = TempTree::new("leaf");
        let closed = tree.dir("site-packages", 0o750);
        let blocker = unreadable(std::slice::from_ref(&closed)).expect("a blocker");
        assert_eq!(blocker.path, closed);
        assert_eq!(blocker.mode, 0o750);
        assert_eq!(blocker.access, Access::Read);
        assert!(blocker.message().contains("0750"), "{}", blocker.message());
        assert!(
            blocker.message().contains(&closed.display().to_string()),
            "{}",
            blocker.message()
        );
        assert!(
            blocker.message().contains("stringprep"),
            "{}",
            blocker.message()
        );

        // A directory others may read but not descend into is traversable's
        // problem, and the message says which of the two it is.
        let opaque = tree.dir("opaque", 0o744);
        let blocker = unreadable(&[opaque]).expect("a blocker");
        assert_eq!(blocker.access, Access::Traverse);
        assert!(
            blocker.message().contains("traversable"),
            "{}",
            blocker.message()
        );
    }

    #[test]
    fn an_ancestor_a_view_could_not_traverse_is_named_not_the_leaf() {
        let tree = TempTree::new("ancestor");
        let private = tree.dir("home", 0o750);
        let inside = private.join("python");
        std::fs::create_dir_all(&inside).unwrap();
        std::fs::set_permissions(&inside, std::fs::Permissions::from_mode(0o755)).unwrap();

        // The interpreter directory itself is open; the home above it is not,
        // and that is the directory an operator has to change.
        let blocker = unreadable(&[inside]).expect("a blocker");
        assert_eq!(blocker.path, private);
        assert_eq!(blocker.access, Access::Traverse);
        assert_eq!(blocker.mode, 0o750);
    }

    #[test]
    fn the_first_blocker_wins_so_the_interpreter_is_reported_first() {
        let tree = TempTree::new("order");
        let first = tree.dir("interpreter", 0o700);
        let second = tree.dir("project", 0o700);
        let blocker = unreadable(&[first.clone(), second]).expect("a blocker");
        assert_eq!(blocker.path, first);
    }

    #[test]
    fn a_private_home_refuses_a_uv_managed_interpreter() {
        // The layout the papercut is about: `~/.local/share/uv/python/...` under
        // a `0750` home, where the interpreter and its own directories are
        // perfectly open — and unreachable, because nobody else may pass the
        // home.
        let tree = TempTree::new("uv");
        let home = tree.dir("me", 0o750);
        let store = home.join(".local/share/uv/python/cpython-3.14");
        std::fs::create_dir_all(&store).unwrap();
        for directory in [
            home.join(".local"),
            home.join(".local/share"),
            home.join(".local/share/uv"),
            home.join(".local/share/uv/python"),
            store.clone(),
        ] {
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let blocker = unreadable(&[
            store,
            home.join(".local/share/uv/python/cpython-3.14/lib/python3.14"),
        ])
        .expect("a blocker");
        assert_eq!(blocker.path, home);
        assert!(
            blocker.message().contains("UV_PYTHON_INSTALL_DIR"),
            "the fix is named: {}",
            blocker.message()
        );
    }
}
