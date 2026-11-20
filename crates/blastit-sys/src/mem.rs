//! Machine memory, from `/proc/meminfo` and the cgroup files (ARCHITECTURE.md
//! §8 Sizing, §9.2 #8).
//!
//! §8 sizes `BLASTIT_MAX_VIEW_PROCESSES` from the memory that is *free*, and
//! §9.2 #8 records where that number comes from on App Platform: `MemTotal`
//! from `/proc/meminfo` is the instance size, while the cgroup's own usage is
//! the accurate one under gVisor (`VmRSS` does not move). So a reading is:
//!
//! ```text
//! total = min(meminfo MemTotal, cgroup limit, if the limit is real)
//! used  = cgroup usage, else meminfo MemTotal − MemAvailable
//! ```
//!
//! Both cgroup layouts are read: v2 (`memory.max`, `memory.current`) and v1
//! (`memory/memory.limit_in_bytes`, `memory/memory.usage_in_bytes`). v2 is
//! preferred when it is there, and a limit the kernel reports as unlimited
//! (`max`, or v1's `LONG_MAX`-rounded value) is treated as no limit at all.
//!
//! [`Reader`] takes its root as a parameter rather than hard-coding `/`, so
//! §8's arithmetic can be tested against files instead of the machine.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::Error;

/// Where `/proc/meminfo` lives under a root.
pub const MEMINFO: &str = "proc/meminfo";

/// cgroup v2's memory limit file.
pub const CGROUP_V2_MAX: &str = "sys/fs/cgroup/memory.max";

/// cgroup v2's memory usage file.
pub const CGROUP_V2_CURRENT: &str = "sys/fs/cgroup/memory.current";

/// cgroup v1's memory limit file.
pub const CGROUP_V1_LIMIT: &str = "sys/fs/cgroup/memory/memory.limit_in_bytes";

/// cgroup v1's memory usage file.
pub const CGROUP_V1_USAGE: &str = "sys/fs/cgroup/memory/memory.usage_in_bytes";

/// At or above this, a cgroup limit is the kernel's way of saying "unlimited".
///
/// cgroup v1 writes unlimited as `LONG_MAX` rounded down to a page boundary
/// (`0x7fff_ffff_ffff_f000`), which is above `2^62`; no real machine has that
/// much memory.
pub const UNLIMITED_AT: u64 = 1 << 62;

/// `MemTotal` and `MemAvailable` of a `/proc/meminfo`, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemInfo {
    total: u64,
    available: Option<u64>,
}

impl MemInfo {
    /// The machine's memory, in bytes.
    pub fn total(&self) -> u64 {
        self.total
    }

    /// The memory the kernel thinks is available, when it says (Linux 3.14+).
    pub fn available(&self) -> Option<u64> {
        self.available
    }
}

/// A cgroup memory limit, as the kernel writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CgroupLimit {
    /// A real limit, in bytes.
    Bytes(u64),
    /// `max`, or v1's page-rounded `LONG_MAX`: not a limit.
    Unlimited,
}

/// One memory reading: what may be used, and how much is in use already.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    total: u64,
    used: u64,
}

impl Sample {
    /// The memory this process may use, in bytes: `min(MemTotal, cgroup
    /// limit)`.
    pub fn total(&self) -> u64 {
        self.total
    }

    /// How much of it is in use now, in bytes.
    ///
    /// This is the **raw** figure: the cgroup's `memory.current`, or
    /// `MemTotal − MemAvailable` without one. It counts page cache the kernel
    /// will hand back on demand, so it is the right answer to "how big is this
    /// cgroup" and the wrong one to "may another process start". That second
    /// question is `blastit`'s `mem_usage` reading — this sample minus the
    /// `memory.stat` bytes the kernel can reclaim — which builds on this one and
    /// adds no second parser of `/proc/meminfo` or the cgroup files.
    pub fn used(&self) -> u64 {
        self.used
    }
}

/// Parses `/proc/meminfo`.
///
/// Returns `None` when `MemTotal` is missing or unreadable; `MemAvailable` is
/// optional because a kernel without it (before 3.14) still has `MemTotal`.
/// The kernel documents every value in the file as kibibytes.
pub fn parse_meminfo(text: &str) -> Option<MemInfo> {
    let mut total = None;
    let mut available = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            total = parse_kib(rest);
        } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
            available = parse_kib(rest);
        }
    }
    Some(MemInfo {
        total: total?,
        available,
    })
}

/// Parses a cgroup limit file (`memory.max`, `memory.limit_in_bytes`).
///
/// Returns `None` for an empty or unparseable file, which is what a root
/// without cgroups looks like.
pub fn parse_cgroup_limit(text: &str) -> Option<CgroupLimit> {
    let text = text.trim();
    if text == "max" {
        return Some(CgroupLimit::Unlimited);
    }
    let value: u64 = text.parse().ok()?;
    if value >= UNLIMITED_AT {
        Some(CgroupLimit::Unlimited)
    } else {
        Some(CgroupLimit::Bytes(value))
    }
}

/// Parses a cgroup usage file (`memory.current`, `memory.usage_in_bytes`).
pub fn parse_cgroup_usage(text: &str) -> Option<u64> {
    text.trim().parse().ok()
}

/// Reads memory figures from a directory tree, so tests can use temp files.
///
/// In production the root is `/`; nothing here is Linux-specific beyond the
/// paths, which is what makes the reader testable.
#[derive(Debug, Clone)]
pub struct Reader {
    root: PathBuf,
}

impl Reader {
    /// A reader rooted at `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The root this reader looks under.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Reads one sample: `total` from `meminfo` and the cgroup limit, `used`
    /// from the cgroup usage or from `MemTotal − MemAvailable`.
    ///
    /// The file reads behind this block, and that is deliberate: every file is
    /// a couple of kilobytes of kernel-generated text that never touches a
    /// disk, and the callers are off the request path (the scheduler's §8
    /// probe, at most once per its 2 s check interval, and the two readings
    /// init takes while it starts).
    ///
    /// # Errors
    ///
    /// An [`io::Error`] when `/proc/meminfo` cannot be read, or when it is
    /// there but has no usable `MemTotal`, or when `used` cannot be derived at
    /// all. §8's caller treats that as "no derived cap".
    pub fn sample(&self) -> io::Result<Sample> {
        let meminfo_path = self.root.join(MEMINFO);
        let text = read(&meminfo_path)?;
        let meminfo = parse_meminfo(&text).ok_or_else(|| Error::MemoryFigure {
            path: meminfo_path.clone(),
            detail: "no MemTotal",
        })?;

        let total = match self.cgroup_limit() {
            Some(CgroupLimit::Bytes(limit)) => meminfo.total().min(limit),
            Some(CgroupLimit::Unlimited) | None => meminfo.total(),
        };
        let used = match self.cgroup_usage() {
            Some(used) => used,
            None => meminfo.available().map_or_else(
                || {
                    Err(Error::MemoryFigure {
                        path: meminfo_path,
                        detail: "no MemAvailable and no cgroup usage",
                    })
                },
                |available| Ok(meminfo.total().saturating_sub(available)),
            )?,
        };
        // Usage can sit above the limit while the kernel reclaims; the cap is
        // arithmetic, so keep the reading sane for it.
        Ok(Sample {
            total,
            used: used.min(total),
        })
    }

    /// The cgroup limit, from v2 or the v1 fallback.
    fn cgroup_limit(&self) -> Option<CgroupLimit> {
        for relative in [CGROUP_V2_MAX, CGROUP_V1_LIMIT] {
            if let Ok(text) = fs::read_to_string(self.root.join(relative))
                && let Some(limit) = parse_cgroup_limit(&text)
            {
                return Some(limit);
            }
        }
        None
    }

    /// The cgroup usage, from v2 or the v1 fallback.
    fn cgroup_usage(&self) -> Option<u64> {
        for relative in [CGROUP_V2_CURRENT, CGROUP_V1_USAGE] {
            if let Ok(text) = fs::read_to_string(self.root.join(relative))
                && let Some(used) = parse_cgroup_usage(&text)
            {
                return Some(used);
            }
        }
        None
    }
}

/// One `/proc/meminfo` value, in bytes.
fn parse_kib(rest: &str) -> Option<u64> {
    let value: u64 = rest.split_whitespace().next()?.parse().ok()?;
    Some(value.saturating_mul(1024))
}

/// Reads a file, keeping the path in the error: the caller's log has to name
/// the file behind §8 Sizing's numbers.
///
/// The read blocks, and that is deliberate. Every file here is a couple of
/// kilobytes of kernel-generated text that never touches a disk, and the
/// callers are §8's memory probe — which the scheduler runs off the request
/// path, at most once per check interval (2 s) — and the two readings init
/// takes while it starts. A request never waits on this.
fn read(path: &Path) -> io::Result<String> {
    fs::read_to_string(path).map_err(|source| {
        Error::MemoryUnreadable {
            path: path.to_path_buf(),
            source,
        }
        .into()
    })
}
