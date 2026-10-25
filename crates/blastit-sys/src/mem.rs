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
/// the file `§8 Sizing` could not read.
fn read(path: &Path) -> io::Result<String> {
    fs::read_to_string(path).map_err(|source| {
        Error::MemoryUnreadable {
            path: path.to_path_buf(),
            source,
        }
        .into()
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    /// A `meminfo` with the two lines this module reads.
    fn meminfo(total_kib: u64, available_kib: u64) -> String {
        format!(
            "MemTotal:       {total_kib} kB\n\
             MemFree:        123456 kB\n\
             MemAvailable:   {available_kib} kB\n\
             Buffers:             0 kB\n"
        )
    }

    /// Writes `relative` under `root`, creating the directories for it.
    fn write(root: &Path, relative: &str, contents: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn meminfo_is_read_in_bytes() {
        let info = parse_meminfo(&meminfo(1_048_576, 524_288)).unwrap();
        assert_eq!(info.total(), 1024 * 1024 * 1024);
        assert_eq!(info.available(), Some(512 * 1024 * 1024));
    }

    #[test]
    fn meminfo_needs_mem_total_only() {
        let text = "MemFree: 10 kB\nMemAvailable: 20 kB\n";
        assert_eq!(parse_meminfo(text), None);
        let text = "MemTotal: 10 kB\n";
        let info = parse_meminfo(text).unwrap();
        assert_eq!(info.total(), 10 * 1024);
        assert_eq!(info.available(), None, "an old kernel has no MemAvailable");
    }

    #[test]
    fn meminfo_ignores_junk() {
        // A non-numeric value, and a line that merely starts the same way.
        assert_eq!(parse_meminfo("MemTotal: not-a-number kB\n"), None);
        assert_eq!(parse_meminfo("MemTotalish: 5 kB\n"), None);
        // Huge but valid values do not overflow: they saturate.
        let info = parse_meminfo("MemTotal: 18446744073709551615 kB\n").unwrap();
        assert_eq!(info.total(), u64::MAX);
    }

    #[test]
    fn cgroup_limits_are_read_or_seen_as_unlimited() {
        assert_eq!(parse_cgroup_limit("max\n"), Some(CgroupLimit::Unlimited));
        assert_eq!(
            parse_cgroup_limit(" 1073741824 "),
            Some(CgroupLimit::Bytes(1024 * 1024 * 1024))
        );
        // cgroup v1's unlimited is LONG_MAX rounded down to a page boundary.
        assert_eq!(
            parse_cgroup_limit("9223372036854771712\n"),
            Some(CgroupLimit::Unlimited)
        );
        assert_eq!(parse_cgroup_limit(""), None);
        assert_eq!(parse_cgroup_limit("\n"), None);
        assert_eq!(parse_cgroup_limit("nonsense"), None);
        assert_eq!(parse_cgroup_limit("-1"), None);
    }

    #[test]
    fn cgroup_usage_is_read() {
        assert_eq!(parse_cgroup_usage("424242\n"), Some(424242));
        assert_eq!(parse_cgroup_usage(""), None);
        assert_eq!(parse_cgroup_usage("max"), None);
    }

    #[test]
    fn a_sample_takes_the_smallest_total_and_the_cgroup_usage() {
        let dir = TempDir::new("mem-v2");
        write(dir.path(), MEMINFO, &meminfo(1_048_576, 900_000));
        write(dir.path(), CGROUP_V2_MAX, "268435456\n");
        write(dir.path(), CGROUP_V2_CURRENT, "67108864\n");
        let sample = Reader::new(dir.path()).sample().unwrap();
        // The cgroup limit is smaller than MemTotal, so it wins.
        assert_eq!(sample.total(), 256 * 1024 * 1024);
        assert_eq!(sample.used(), 64 * 1024 * 1024);
    }

    #[test]
    fn an_unlimited_cgroup_falls_back_to_mem_info() {
        let dir = TempDir::new("mem-v2-max");
        write(dir.path(), MEMINFO, &meminfo(1_048_576, 900_000));
        write(dir.path(), CGROUP_V2_MAX, "max\n");
        write(dir.path(), CGROUP_V2_CURRENT, "1048576\n");
        let sample = Reader::new(dir.path()).sample().unwrap();
        assert_eq!(sample.total(), 1024 * 1024 * 1024);
        assert_eq!(sample.used(), 1024 * 1024);
    }

    #[test]
    fn without_cgroups_usage_comes_from_mem_available() {
        let dir = TempDir::new("mem-nocgroup");
        write(dir.path(), MEMINFO, &meminfo(1_048_576, 262_144));
        let sample = Reader::new(dir.path()).sample().unwrap();
        assert_eq!(sample.total(), 1024 * 1024 * 1024);
        assert_eq!(sample.used(), 768 * 1024 * 1024, "MemTotal − MemAvailable");
    }

    #[test]
    fn cgroup_v1_files_are_read_too() {
        let dir = TempDir::new("mem-v1");
        write(dir.path(), MEMINFO, &meminfo(1_048_576, 900_000));
        write(dir.path(), CGROUP_V1_LIMIT, "9223372036854771712\n");
        write(dir.path(), CGROUP_V1_USAGE, "12345678\n");
        let sample = Reader::new(dir.path()).sample().unwrap();
        assert_eq!(sample.total(), 1024 * 1024 * 1024, "v1 unlimited");
        assert_eq!(sample.used(), 12_345_678);
    }

    #[test]
    fn cgroup_v2_wins_over_v1() {
        let dir = TempDir::new("mem-both");
        write(dir.path(), MEMINFO, &meminfo(1_048_576, 900_000));
        write(dir.path(), CGROUP_V2_MAX, "1073741824\n");
        write(dir.path(), CGROUP_V2_CURRENT, "1073741824\n");
        write(dir.path(), CGROUP_V1_LIMIT, "536870912\n");
        write(dir.path(), CGROUP_V1_USAGE, "1\n");
        let sample = Reader::new(dir.path()).sample().unwrap();
        assert_eq!(
            (sample.total(), sample.used()),
            (1024 * 1024 * 1024, 1024 * 1024 * 1024)
        );
    }

    #[test]
    fn usage_above_the_limit_is_clamped() {
        let dir = TempDir::new("mem-over");
        write(dir.path(), MEMINFO, &meminfo(1_048_576, 900_000));
        write(dir.path(), CGROUP_V2_MAX, "104857600\n");
        write(dir.path(), CGROUP_V2_CURRENT, "134217728\n");
        let sample = Reader::new(dir.path()).sample().unwrap();
        assert_eq!(sample.used(), sample.total());
    }

    #[test]
    fn a_missing_or_broken_meminfo_is_an_error() {
        let dir = TempDir::new("mem-missing");
        let err = Reader::new(dir.path()).sample().unwrap_err();
        assert!(err.to_string().contains("proc/meminfo"), "{err}");

        let dir = TempDir::new("mem-broken");
        write(dir.path(), MEMINFO, "MemFree: 1 kB\n");
        let err = Reader::new(dir.path()).sample().unwrap_err();
        let typed = Error::from_io(&err).expect("a typed error");
        assert!(
            matches!(typed, Error::MemoryFigure { detail, .. } if *detail == "no MemTotal"),
            "{typed}"
        );
    }

    #[test]
    fn a_sample_without_usage_anywhere_is_an_error() {
        let dir = TempDir::new("mem-nousage");
        write(dir.path(), MEMINFO, "MemTotal: 1024 kB\n");
        let err = Reader::new(dir.path()).sample().unwrap_err();
        let typed = Error::from_io(&err).expect("a typed error");
        assert!(
            matches!(typed, Error::MemoryFigure { detail, .. } if detail.contains("MemAvailable")),
            "{typed}"
        );
    }

    #[test]
    fn the_real_machine_reads() {
        let sample = Reader::new("/").sample().unwrap();
        assert!(sample.total() > 0, "a machine with no memory");
        assert!(sample.used() <= sample.total());
    }
}
