//! `TESTING_PLAN.md` §6 — fail-closed runtime integrity: a shared object whose
//! identity moved after blastit inspected it is refused, in both shapes the boot
//! reaches.
//!
//! Two checks carry that rule, and this file is their witness:
//!
//! 1. **The pin itself** (`blastit_sys::pin::open_pinned`, `pin.rs:62-63`), which
//!    `stage0/fork_exec.rs` calls for every Python boot before the `execve`: the
//!    path was replaced between "blastit inspected it" and the open that pins it.
//! 2. **The live descriptor** — the `fstat`-against-the-record comparison
//!    `stage1/adopt.rs:316-328` runs on the descriptor stage 0 handed over. A real
//!    `dup2` of another file onto that descriptor number is the only way to
//!    produce the state, and it cannot be driven end-to-end: stage 0 pins and
//!    stage 1 re-checks inside one `execve` chain, whose only entry point is the
//!    internal `--stage=1 --config-fd` grammar with a hand-written configuration
//!    document. So the mechanism is driven here, against the same `(st_dev,
//!    st_ino)` pair both sides compare, with real files and real descriptors.
//!
//! No `Mock*`, no `#[ignore]`, no skip: every case is a real file on a real
//! filesystem under `/tmp/opencode`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use blastit_sys::pin::{PinError, SharedObject};

/// A directory with a name of its own, removed when dropped.
struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Self {
        let base = Path::new("/tmp/opencode");
        let base = if std::fs::create_dir_all(base).is_ok() {
            base.to_path_buf()
        } else {
            std::env::temp_dir()
        };
        let dir = base.join(format!("blastit-pin-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the fixture directory");
        Self { dir }
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.dir.join(name);
        std::fs::write(&path, bytes).expect("a fixture file");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The `(st_dev, st_ino)` a path names right now.
fn identity_of(path: &Path) -> (u64, u64) {
    let meta = std::fs::metadata(path).expect("a fixture file's metadata");
    (meta.dev(), meta.ino())
}

/// §6: a path replaced after inspection is refused with both inodes and the fix,
/// while the descriptor already pinned keeps the file it inspected.
#[test]
fn a_path_replaced_after_inspection_is_refused_with_both_inodes() {
    let scratch = Scratch::new("replace");
    let inspected = scratch.write("libpython3.14.so.1.0", b"the inspected object");
    let replacement = scratch.write("replacement", b"a different object");
    let pinned = SharedObject::open(&inspected, None).expect("a regular file pins");
    let record = pinned.identity().expect("the identity stage 0 records");
    let replacement_record = identity_of(&replacement);
    assert_ne!(
        record.1, replacement_record.1,
        "the fixture needs two identities"
    );

    // `mv replacement libpython…`, which is what deploying a rebuilt library does.
    std::fs::rename(&replacement, &inspected).expect("replace the path");

    let refused = SharedObject::open(&inspected, Some(record))
        .expect_err("the path no longer names the object that was inspected");
    let PinError::IdentityChanged { .. } = &refused else {
        panic!("the refusal is the identity change, not {refused:?}");
    };
    assert_eq!(
        refused.to_string(),
        format!(
            "{} was replaced after blastit inspected it (inode {} is now {}); restore the file \
             and restart blastit",
            inspected.display(),
            record.1,
            replacement_record.1
        ),
        "the refusal names both identities and the fix"
    );

    // Accepted twin: recording the identity the path *now* names pins it.
    SharedObject::open(&inspected, Some(replacement_record))
        .expect("the object the path now names pins");

    // And the descriptor pinned before the swap still names the inspected file:
    // nothing re-opens by path after the pin.
    assert_eq!(
        pinned.identity().expect("the pinned identity"),
        record,
        "the pin keeps the file it opened"
    );
}

/// §6: a descriptor whose file was swapped under it (a real `dup2`, the state
/// `stage1/adopt.rs` refuses) no longer matches the identity stage 0 recorded.
#[test]
fn a_descriptor_swapped_under_the_pin_reports_the_new_inode() {
    let scratch = Scratch::new("dup2");
    let inspected = scratch.write("libpython3.14.so.1.0", b"the inspected object");
    let replacement = scratch.write("replacement", b"a different object");
    let pinned = SharedObject::open(&inspected, None).expect("a regular file pins");
    let record = pinned.identity().expect("the identity stage 0 records");
    let replacement_record = identity_of(&replacement);
    assert_ne!(record, replacement_record, "the fixture needs two files");

    // The real swap: the number stage 0 recorded now names another file, so the
    // `fstat` stage 1 performs reads a different identity from its record.
    let swapped = std::fs::File::open(&replacement).expect("the replacement opens");
    // SAFETY: both descriptors are open in this process; `dup2` atomically makes
    // the pinned number name the replacement and closes its old description.
    let rc = unsafe { libc::dup2(swapped.as_raw_fd(), pinned.as_raw_fd()) };
    assert!(rc >= 0, "dup2: {}", std::io::Error::last_os_error());

    assert_eq!(
        pinned.identity().expect("the swapped identity"),
        replacement_record,
        "the live descriptor reports its new file"
    );
    assert_ne!(
        pinned.identity().expect("the swapped identity"),
        record,
        "the recorded identity and the live descriptor disagree, which is the refusal"
    );
    // Only the descriptor betrays the swap: the file the record names is intact,
    // so a boot that compared paths instead would serve the wrong object.
    SharedObject::open(&inspected, Some(record))
        .expect("the file the record names is still the inspected one");
    SharedObject::open(&inspected, Some(replacement_record))
        .expect_err("pinning the path against the wrong identity is refused");
}
