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

#[cfg(test)]
mod tests {
    use std::os::fd::AsRawFd;

    use super::*;
    use crate::Error;
    use crate::pid::ExitStatus;
    use crate::testutil::{in_child, must, must_some, read_to_end, wait_for};

    /// Run `path` in a forked child with stdout on a pipe and return its output.
    fn exec_capture(
        path: &CStr,
        argv: &[CString],
        env: &[(String, String)],
    ) -> (Vec<u8>, ExitStatus) {
        let (read_end, write_end) = must(crate::pipe());
        // SAFETY: the child only `dup2`s, `exec`s and `_exit`s.
        let fork = must(unsafe { crate::fork_unchecked() });
        match fork {
            crate::Fork::Child => {
                // SAFETY: both descriptors are open in this process, and `1` is
                // a valid target for `dup2`.
                unsafe { libc::dup2(write_end.as_raw_fd(), 1) };
                drop((read_end, write_end));
                // On success this does not come back.
                let _ = exec(path, argv, env);
                // SAFETY: `_exit` never returns.
                unsafe { libc::_exit(127) }
            }
            crate::Fork::Parent(pid) => {
                drop(write_end);
                let output = read_to_end(read_end);
                (output, wait_for(pid))
            }
        }
    }

    /// `/usr/bin/env`, or a skip when this system has no such binary.
    fn env_program() -> Option<(&'static CStr, Vec<CString>)> {
        if !Path::new("/usr/bin/env").exists() {
            println!("skipped: needs /usr/bin/env");
            return None;
        }
        let path = c"/usr/bin/env";
        let argv = vec![must_some(CString::new("env").ok())];
        Some((path, argv))
    }

    #[test]
    fn exe_path_is_the_procfs_entry_of_the_running_binary() {
        assert_eq!(exe_path(), Path::new("/proc/self/exe"));
        let resolved = must(std::fs::read_link(exe_path()));
        let current = must(std::env::current_exe());
        assert_eq!(
            must(std::fs::canonicalize(resolved)),
            must(std::fs::canonicalize(current))
        );
    }

    #[test]
    fn exec_with_an_empty_environment_leaves_the_child_with_no_environment() {
        let Some((path, argv)) = env_program() else {
            return;
        };
        let (output, status) = exec_capture(path, &argv, &[]);
        assert_eq!(status, ExitStatus::Code(0));
        assert!(
            output.is_empty(),
            "the new image inherited an environment: {}",
            String::from_utf8_lossy(&output)
        );
    }

    #[test]
    fn exec_with_a_given_environment_sets_exactly_that() {
        let Some((path, argv)) = env_program() else {
            return;
        };
        let env = vec![
            ("BLASTIT_ONE".to_string(), "1".to_string()),
            ("BLASTIT_TWO".to_string(), "two words".to_string()),
        ];
        let (output, status) = exec_capture(path, &argv, &env);
        assert_eq!(status, ExitStatus::Code(0));
        let text = String::from_utf8_lossy(&output);
        let mut lines: Vec<&str> = text.lines().collect();
        lines.sort_unstable();
        assert_eq!(lines, vec!["BLASTIT_ONE=1", "BLASTIT_TWO=two words"]);
        assert!(
            !text.contains("PATH="),
            "nothing of the parent's environment is passed on"
        );
    }

    #[test]
    fn exec_reports_a_program_that_is_not_there() {
        let code = in_child(|| {
            let path = match CString::new("/nonexistent-blastit-sys") {
                Ok(path) => path,
                Err(_) => return 3,
            };
            let err = exec(&path, &[], &[]);
            match err.kind() {
                io::ErrorKind::NotFound => 0,
                _ => 1,
            }
        });
        assert_eq!(code, 0, "child failure code {code}");
    }

    #[test]
    fn exec_rejects_a_bad_environment_before_it_executes_anything() {
        // The path does not exist, so an environment that passed validation
        // would fail with `NotFound`; anything that comes back as
        // `InvalidEnv` proves the check ran first.
        let code = in_child(|| {
            let path = match CString::new("/nonexistent-blastit-sys") {
                Ok(path) => path,
                Err(_) => return 3,
            };
            let argv = vec![match CString::new("nothing") {
                Ok(arg) => arg,
                Err(_) => return 4,
            }];
            let cases = [
                vec![("BAD=KEY".to_string(), "v".to_string())],
                vec![(String::new(), "v".to_string())],
                vec![("KEY".to_string(), "va\0lue".to_string())],
                vec![("KE\0Y".to_string(), "v".to_string())],
            ];
            for env in cases {
                let err = exec(&path, &argv, &env);
                match Error::from_io(&err) {
                    Some(Error::InvalidEnv { .. }) => {}
                    _ => return 5,
                }
            }
            0
        });
        assert_eq!(code, 0, "child failure code {code}");
    }

    #[test]
    fn exec_accepts_an_environment_that_is_only_a_name() {
        // A value-less entry is legal: `KEY=` with an empty value.
        let code = in_child(|| {
            let path = match CString::new("/nonexistent-blastit-sys") {
                Ok(path) => path,
                Err(_) => return 3,
            };
            let err = exec(
                &path,
                &[],
                &[
                    ("EMPTY".to_string(), String::new()),
                    ("V".to_string(), "1".to_string()),
                ],
            );
            match err.kind() {
                io::ErrorKind::NotFound => 0,
                _ => 1,
            }
        });
        assert_eq!(code, 0, "child failure code {code}");
    }
}
