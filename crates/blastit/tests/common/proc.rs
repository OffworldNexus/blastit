//! The `/proc` probes the shutdown leg reads: command line, `--dev-root`,
//! descendants and start times, kept out of the suite body.

use std::path::{Path, PathBuf};

/// The command line of `pid`, or a panic naming the process.
pub fn cmdline(pid: u32) -> String {
    let bytes = std::fs::read(format!("/proc/{pid}/cmdline"))
        .unwrap_or_else(|err| panic!("reading the cmdline of {pid}: {err}"));
    String::from_utf8_lossy(&bytes)
        .split('\0')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The `--dev-root` stage 1 was given.
pub fn dev_root(pid: u32) -> PathBuf {
    let cmdline = cmdline(pid);
    let root = cmdline
        .split_whitespace()
        .find_map(|arg| arg.strip_prefix("--dev-root="))
        .unwrap_or_else(|| panic!("no --dev-root in {cmdline:?}"));
    PathBuf::from(root)
}

/// Every process `pid` forked, recursively, with its `starttime`.
pub fn descendants(pid: u32) -> Vec<(i32, u64)> {
    let mut found = Vec::new();
    let mut frontier = vec![pid as i32];
    while let Some(pid) = frontier.pop() {
        for child in children_of(pid) {
            if let Some(starttime) = starttime(child) {
                found.push((child, starttime));
                frontier.push(child);
            }
        }
    }
    found
}

/// The pids of `pid`'s children, from the kernel's own list.
pub fn children_of(pid: i32) -> Vec<i32> {
    let mut children = Vec::new();
    let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) else {
        return children;
    };
    for task in tasks.flatten() {
        if let Ok(text) = std::fs::read_to_string(task.path().join("children")) {
            children.extend(
                text.split_whitespace()
                    .filter_map(|pid| pid.parse::<i32>().ok()),
            );
        }
    }
    children
}

/// Field 22 of `/proc/<pid>/stat` (the start time), or `None` when it is gone.
pub fn starttime(pid: i32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // `comm` is parenthesised and may contain spaces, so fields resume after the
    // *last* `)`.
    let rest = stat.rsplit_once(')')?.1;
    rest.split_whitespace().nth(19)?.parse().ok()
}

/// Whether `pid` still has a `/proc` entry.
pub fn in_proc(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}
