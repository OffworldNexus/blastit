//! This run's private runtime root, and the descriptor helpers it shares.
//!
//! Everything `check` writes lives under one `0700` directory under `$TMPDIR`:
//! the observation file the gate appends to, the `pg` socket directory
//! `swapped_env` renders, and the one-shot Django handshake. It is removed when
//! [`CheckRoot`] is dropped, so a run leaves nothing behind on any path out.

use std::os::fd::{OwnedFd, RawFd};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use super::error::ObsError;

/// This run's private `0700` runtime root, removed when it is dropped.
pub struct CheckRoot {
    path: PathBuf,
}

impl CheckRoot {
    /// Creates `$TMPDIR/blastit-check-<uid>-<pid>-<nonce>`, `0700`, owned by
    /// this uid.
    pub fn create() -> Result<Self, String> {
        let uid = rustix::process::geteuid().as_raw();
        let base = std::env::temp_dir();
        let mut last = None;
        for attempt in 0..16u32 {
            let name = format!(
                "blastit-check-{uid}-{}-{}-{attempt}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|elapsed| elapsed.subsec_nanos())
                    .unwrap_or(0)
            );
            let path = base.join(name);
            match rustix::fs::mkdir(&path, rustix::fs::Mode::from_bits_truncate(0o700)) {
                Ok(()) => {
                    verify_private(&path, uid)?;
                    return Ok(Self { path });
                }
                Err(error) => last = Some(error),
            }
        }
        Err(format!(
            "{}: could not create a private runtime directory: {}",
            base.display(),
            last.map(|error| error.to_string()).unwrap_or_default()
        ))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Creates and returns `<root>/pg`, the socket directory `swapped_env`
    /// renders.
    pub fn postgres_dir(&self) -> Result<PathBuf, String> {
        let dir = self.path.join("pg");
        rustix::fs::mkdir(&dir, rustix::fs::Mode::from_bits_truncate(0o700))
            .map_err(|error| format!("{}: {error}", dir.display()))?;
        verify_private(&dir, rustix::process::geteuid().as_raw())?;
        Ok(dir)
    }
}

impl Drop for CheckRoot {
    fn drop(&mut self) {
        let Ok(metadata) = std::fs::symlink_metadata(&self.path) else {
            return;
        };
        if !metadata.file_type().is_dir() || metadata.uid() != rustix::process::geteuid().as_raw() {
            return;
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Checks that `path` is a `0700` directory owned by `uid`.
fn verify_private(path: &Path, uid: u32) -> Result<(), String> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if !metadata.file_type().is_dir() || metadata.uid() != uid {
        return Err(format!(
            "{}: not a directory owned by uid {uid}",
            path.display()
        ));
    }
    if metadata.permissions().mode() & 0o777 != 0o700 {
        return Err(format!("{}: is not 0700", path.display()));
    }
    Ok(())
}

/// Names the file an observation line came from (`T2-M11`).
pub fn observation_error(runtime: &CheckRoot, error: ObsError) -> String {
    let http = runtime.path().join("http_obs.jsonl");
    match error {
        ObsError::Malformed { file, line, reason } => format!("{file}: line {line}: {reason}"),
        other => {
            let hint = if http.is_file() {
                format!(" (the file this run's gate wrote is {})", http.display())
            } else {
                String::new()
            };
            format!("{other}{hint}")
        }
    }
}

/// A socketpair, with a one-line error naming the channel.
pub fn socketpair(channel: &str) -> Result<(OwnedFd, OwnedFd), String> {
    let (a, b) = UnixStream::pair().map_err(|error| format!("{channel}: {error}"))?;
    Ok((OwnedFd::from(a), OwnedFd::from(b)))
}

/// Clears `FD_CLOEXEC` so the child inherits `fd` across its `execve`.
pub fn clear_cloexec(fd: RawFd) -> Result<(), String> {
    // SAFETY: `fd` is one of this run's own descriptors and this is the only
    // place that changes its flags.
    let borrowed = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) };
    rustix::io::fcntl_setfd(borrowed, rustix::io::FdFlags::empty())
        .map_err(|error| format!("descriptor {fd}: {error}"))
}

/// A duplicate of stderr, for a child whose stdout must not be stdout.
pub fn stdio_stderr() -> Result<Stdio, String> {
    let duplicated = rustix::io::dup(std::io::stderr())
        .map_err(|error| format!("duplicating stderr: {error}"))?;
    Ok(Stdio::from(duplicated))
}
