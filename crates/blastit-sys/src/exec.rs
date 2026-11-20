//! `execve` and the path init re-executes itself from.

use std::ffi::{CStr, CString};
use std::io;
use std::path::Path;
use std::ptr;

use crate::Error;

/// The path init re-executes itself from when it crosses the `execve` barrier.
///
/// The address space is replaced, so nothing a secret was ever written to
/// survives, and PID 1 stays PID 1 with dbproxy still its child (§4).
pub fn exe_path() -> &'static Path {
    Path::new("/proc/self/exe")
}

/// Replace this process image with `path`, with exactly `argv` and `env`.
///
/// On success this never returns. On failure it returns the `errno`, and the
/// caller is still the old process — which for init means "exit non-zero": a
/// half-started sandbox is worse than a crash loop the platform can see (§4).
///
/// `env` becomes `envp` verbatim. An **empty** slice produces an **empty**
/// environment, not an inherited one: dbproxy is started that way, so its
/// `/proc/<pid>/environ` is empty and the credentials it needs arrive over
/// `secrets.pipe` instead (§4). Each entry is rendered as `key=value`; a key
/// that is empty or contains `=`, or a key or value containing a NUL byte, is
/// rejected with [`Error::InvalidEnv`] before anything is executed.
///
/// `argv` is passed as given, with no implicit `argv[0]`; callers pass at least
/// one argument.
///
/// # What this call needs from its caller
///
/// * **It allocates.** Building the `argv` and `envp` pointer arrays means
///   `Vec`s, so this may only be called in a child of a process that was
///   single-threaded at `fork` time: in a child forked from a multi-threaded
///   process, taking a lock that another thread held at fork time deadlocks
///   forever. That is the same rule that makes init single-threaded at every
///   `fork()` (§2.1), and it is why the C1b binary forks only from init.
/// * **`execve` keeps the signal mask.** Handlers are reset to their default
///   dispositions, but a blocked signal stays blocked across the `execve`, and
///   so does an ignored one. A child that inherited init's `signalfd` mask must
///   put it back itself, with
///   [`restore_signal_mask_in_child`](crate::restore_signal_mask_in_child),
///   before calling this — otherwise the new program starts with `SIGCHLD`,
///   `SIGTERM`, `SIGINT` and `SIGHUP` blocked and no handler for them.
/// * It does not touch descriptors. Whatever a child means to inherit is
///   arranged before the call (`dup_onto`, `close_fds_except`), because the
///   descriptors without `FD_CLOEXEC` are exactly the ones that survive.
pub fn exec(path: &CStr, argv: &[CString], env: &[(String, String)]) -> io::Error {
    let mut argv_ptrs: Vec<*const libc::c_char> = Vec::with_capacity(argv.len() + 1);
    for arg in argv {
        argv_ptrs.push(arg.as_ptr());
    }
    argv_ptrs.push(ptr::null());

    let mut env_strings: Vec<CString> = Vec::with_capacity(env.len());
    for (index, (key, value)) in env.iter().enumerate() {
        let reason = if key.is_empty() {
            Some("the variable name is empty")
        } else if key.contains('=') {
            Some("the variable name contains `=`")
        } else if key.as_bytes().contains(&0) {
            Some("the variable name contains a NUL byte")
        } else if value.as_bytes().contains(&0) {
            Some("the value contains a NUL byte")
        } else {
            None
        };
        if let Some(reason) = reason {
            return Error::InvalidEnv { index, reason }.into();
        }
        let mut entry = Vec::with_capacity(key.len() + value.len() + 2);
        entry.extend_from_slice(key.as_bytes());
        entry.push(b'=');
        entry.extend_from_slice(value.as_bytes());
        match CString::new(entry) {
            Ok(entry) => env_strings.push(entry),
            Err(_) => {
                return Error::InvalidEnv {
                    index,
                    reason: "the entry contains a NUL byte",
                }
                .into();
            }
        }
    }
    let mut env_ptrs: Vec<*const libc::c_char> = Vec::with_capacity(env_strings.len() + 1);
    for value in &env_strings {
        env_ptrs.push(value.as_ptr());
    }
    env_ptrs.push(ptr::null());

    // SAFETY: `path` is a valid NUL-terminated C string; `argv_ptrs` and
    // `env_ptrs` are NUL-terminated arrays of valid pointers to the `CString`s
    // above, which are alive for the duration of the call. `execve` reads them
    // and either replaces the image or returns.
    unsafe {
        libc::execve(path.as_ptr(), argv_ptrs.as_ptr(), env_ptrs.as_ptr());
    }
    io::Error::last_os_error()
}
