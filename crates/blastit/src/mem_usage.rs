//! The memory reading `BLASTIT_MEM_HIGH_WATERMARK` is compared against
//! (`ARCHITECTURE.md` §8 Sizing, §9.2 #8).
//!
//! §8 asks one question of a reading: *may another view process start?* The
//! figure it must not use is the cgroup's raw `memory.current`, because that
//! counts page cache the kernel will hand back the moment a process needs it.
//! On a host whose filesystem cache is large — a CI runner, a laptop with a
//! warm working set, any machine that just built something — `memory.current`
//! sits near the limit while most of it is reclaimable, and a watermark
//! compared against it refuses to boot a machine that has gigabytes free.
//!
//! So the reading here is the kernel's own split:
//!
//! ```text
//! used  = memory.current − page cache that is not tmpfs − slab_reclaimable
//!       = memory.current − (file − shmem − file_mapped) − slab_reclaimable
//! total = min(meminfo MemTotal, cgroup limit)
//! ```
//!
//! Everything in that subtrahend is memory the kernel gives up on demand:
//! clean and dirty file cache (the LRU's *active* half included — "active"
//! only means "reclaim this second rather than first", and the kernel evicts
//! it under pressure exactly as it does the inactive half), and
//! `slab_reclaimable` (dentries and inodes). What stays counted is memory the
//! kernel cannot take back without swapping: anonymous memory, `shmem`/tmpfs,
//! the page cache mapped into a process, and unreclaimable slab. On a host
//! with a warm cache that is the difference between `memory.current`'s 96 %
//! and the 77 % the same kernel reports as available — the difference between
//! refusing to boot a healthy machine and serving.
//!
//! Two fallbacks, both deliberate: a `memory.stat` without `file` and `shmem`
//! is read with `inactive_file` (+ `slab_reclaimable`) instead, the field older
//! kernels write; a cgroup v1 tree keeps the raw `memory.current` (v1 spells
//! every field differently, and a figure that cannot be read is not pressure);
//! and without cgroups at all the reading is `/proc/meminfo`'s
//! `MemTotal − MemAvailable`, which already leaves reclaimable cache out and so
//! is used as it is.
//!
//! [`read`] takes its root as a parameter rather than hard-coding `/`, so the
//! arithmetic can be pinned against files instead of the machine that runs the
//! test.

use std::fs;
use std::path::Path;

use blastit_sys::mem;

/// The documented default of `BLASTIT_MEM_HIGH_WATERMARK`: above this share of
/// the machine's memory no view process is started (§8 Sizing).
pub const DEFAULT_WATERMARK_PERCENT: f64 = 85.0;

/// cgroup v2's per-controller memory statistics.
pub const CGROUP_V2_STAT: &str = "sys/fs/cgroup/memory.stat";

/// One memory reading: how much memory there is, how much the cgroup says is
/// in use, and how much of that the kernel can reclaim on demand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    total: u64,
    raw: u64,
    reclaimable: u64,
}

impl Usage {
    /// A reading from its three figures: how much memory there is, the raw
    /// usage, and how much of that usage the kernel can reclaim. [`Usage::used`]
    /// and [`Usage::percent`] are arithmetic over them, so a caller that has
    /// the figures — a test fixture, or a host whose files are not the ones
    /// this module reads — gets exactly the production arithmetic.
    pub const fn new(total: u64, raw: u64, reclaimable: u64) -> Self {
        Self {
            total,
            raw,
            reclaimable,
        }
    }

    /// The memory this process may use, in bytes: `min(MemTotal, cgroup
    /// limit)`.
    pub fn total(&self) -> u64 {
        self.total
    }

    /// The figure the cgroup (or `/proc/meminfo`) calls "used", before the
    /// reclaimable part is taken out.
    pub fn raw(&self) -> u64 {
        self.raw
    }

    /// The bytes [`Usage::raw`] counts that the kernel can reclaim on demand:
    /// the page cache that is not tmpfs and not mapped (`file − shmem −
    /// file_mapped`) plus `slab_reclaimable`, and zero when the cgroup cannot
    /// report them (see [`parse_reclaimable`]).
    pub fn reclaimable(&self) -> u64 {
        self.reclaimable
    }

    /// The usage the watermark is compared against: `raw − reclaimable`,
    /// floored at zero.
    pub fn used(&self) -> u64 {
        self.raw.saturating_sub(self.reclaimable)
    }

    /// [`Usage::used`] as a percentage of [`Usage::total`], and zero for a
    /// reading with no total.
    pub fn percent(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.used() as f64 / self.total as f64 * 100.0
        }
    }
}

/// Parses a cgroup `memory.stat` and returns the bytes it marks reclaimable:
/// the page cache that is not tmpfs and not mapped into a process
/// (`file − shmem − file_mapped`) plus `slab_reclaimable`.
///
/// `inactive_file` (+ `slab_reclaimable`) is the fallback for a `memory.stat`
/// with neither `file` nor `shmem`, the pair the subtraction above needs.
/// Returns `None` when the text names neither pair, so a `memory.stat` this
/// module cannot read is "no reclaimable figure" rather than "nothing is
/// reclaimable" — the caller then keeps the raw usage.
pub fn parse_reclaimable(text: &str) -> Option<u64> {
    let mut file = None;
    let mut shmem = None;
    let mut file_mapped = None;
    let mut inactive_file = None;
    let mut slab_reclaimable = None;
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let (Some(name), Some(value)) = (fields.next(), fields.next()) else {
            continue;
        };
        let Ok(value) = value.parse::<u64>() else {
            continue;
        };
        match name {
            "file" => file = Some(value),
            "shmem" => shmem = Some(value),
            "file_mapped" => file_mapped = Some(value),
            "inactive_file" => inactive_file = Some(value),
            "slab_reclaimable" => slab_reclaimable = Some(value),
            _ => {}
        }
    }
    let slab = slab_reclaimable.unwrap_or(0);
    let cache = match (file, shmem) {
        (Some(file), Some(shmem)) => file
            .saturating_sub(shmem)
            .saturating_sub(file_mapped.unwrap_or(0)),
        // `shmem` missing means the split cannot be taken apart safely: a
        // tmpfs page is not reclaimable without swap, so it is never guessed
        // away.
        _ => inactive_file?,
    };
    Some(cache.saturating_add(slab))
}

/// Reads one usage reading from `root` (`/` in production).
///
/// `None` when the machine's memory cannot be read at all, which every caller
/// treats as "no reading", never as pressure (§9.2 #8). See [`sample`] when the
/// reason matters — a boot that reports the failure names the file.
pub fn read(root: &Path) -> Option<Usage> {
    sample(root).ok()
}

/// [`read`] with the reason a reading could not be taken.
///
/// # Errors
///
/// Whatever [`blastit_sys::mem::Reader::sample`] returns: an unreadable
/// `/proc/meminfo`, a missing `MemTotal`, or no way to derive usage at all. The
/// reclaimable figures are best effort — a `memory.stat` that cannot be parsed
/// leaves the raw usage in place rather than failing the reading.
pub fn sample(root: &Path) -> std::io::Result<Usage> {
    let sample = mem::Reader::new(root).sample()?;
    Ok(Usage {
        total: sample.total(),
        raw: sample.used(),
        reclaimable: reclaimable(root),
    })
}

/// The bytes cgroup v2 says are reclaimable under `root`, or zero when it
/// cannot say.
///
/// Only cgroup v2 is subtracted from: its `memory.current` and
/// `memory.stat` sit side by side and use the names this module documents,
/// while v1 spells the fields `total_inactive_file`/`total_slab_reclaimable`
/// and is left as raw usage rather than half-read. Without a cgroup the
/// reading came from `/proc/meminfo`, whose `MemAvailable` already excludes
/// reclaimable cache, so subtracting again would count it twice.
fn reclaimable(root: &Path) -> u64 {
    if fs::read_to_string(root.join(mem::CGROUP_V2_CURRENT)).is_err() {
        return 0;
    }
    fs::read_to_string(root.join(CGROUP_V2_STAT))
        .ok()
        .and_then(|text| parse_reclaimable(&text))
        .unwrap_or(0)
}

/// The sentence a boot above `BLASTIT_MEM_HIGH_WATERMARK` refuses with, or
/// `None` when a view process can start (§8 Sizing).
///
/// The arithmetic is the scheduler's (a percentage of the machine's `total`,
/// taken from [`Usage::used`] so reclaimable cache is not counted), and the
/// message names the setting, the reading and the fix, never a value from the
/// environment (§1.1).
pub fn boot_refusal(usage: &Usage, percent: f64) -> Option<String> {
    let used = usage.used();
    let total = usage.total();
    let watermark = (total as f64 * percent / 100.0) as u64;
    if used <= watermark {
        return None;
    }
    let excluded = match usage.reclaimable() {
        0 => String::new(),
        reclaimable => format!(
            ", counting what the kernel cannot reclaim ({} of page cache and slab left out)",
            gibibytes(reclaimable)
        ),
    };
    Some(format!(
        "memory: {} of {} used ({:.1}%){excluded} is above \
         BLASTIT_MEM_HIGH_WATERMARK ({percent}%): a view process cannot start, so \
         every request would wait out BLASTIT_QUEUE_TIMEOUT and answer 503; free \
         memory or set BLASTIT_MEM_HIGH_WATERMARK higher",
        gibibytes(used),
        gibibytes(total),
        usage.percent(),
    ))
}

/// The one-line `WARN` a `dev` boot prints when it starts above the documented
/// default, or `None` when it does not (§1.1).
///
/// A boot above the default is legitimate once the operator raised
/// `BLASTIT_MEM_HIGH_WATERMARK`: it is their machine and their call. It is
/// also the case where a view process may have to wait for memory before it is
/// forked, so it says so once, naming the setting, the measured value and the
/// reclaimable cache the reading left out.
pub fn advisory(usage: &Usage, percent: f64) -> Option<String> {
    let measured = usage.percent();
    if measured <= DEFAULT_WATERMARK_PERCENT || measured > percent {
        return None;
    }
    Some(format!(
        "memory: {:.1}% of {} used ({}), leaving out {} of reclaimable page cache and \
         slab: that is above the {}% BLASTIT_MEM_HIGH_WATERMARK default, so a view \
         process may have to wait for memory; free memory to bring it down, or set \
         BLASTIT_MEM_HIGH_WATERMARK higher to accept it",
        measured,
        gibibytes(usage.total()),
        gibibytes(usage.used()),
        gibibytes(usage.reclaimable()),
        DEFAULT_WATERMARK_PERCENT,
    ))
}

/// `bytes` as GiB with one decimal, for a human reading of the messages above.
fn gibibytes(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const GIB: u64 = 1024 * 1024 * 1024;

    /// A `memory.stat` in the shape the kernel writes it, in bytes:
    /// `file`, `shmem`, `file_mapped`, `inactive_file`, `slab_reclaimable`.
    fn stat(file: u64, shmem: u64, file_mapped: u64, inactive_file: u64, slab: u64) -> String {
        format!(
            "anon 1073741824\n\
             file {file}\n\
             kernel 1610612736\n\
             shmem {shmem}\n\
             file_mapped {file_mapped}\n\
             inactive_anon 19600384000\n\
             active_anon 18494058496\n\
             inactive_file {inactive_file}\n\
             active_file 4332204032\n\
             slab_reclaimable {slab}\n\
             slab_unreclaimable 292206408\n"
        )
    }

    fn tree(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("blastit-mem-usage-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("proc")).unwrap();
        fs::create_dir_all(dir.join("sys/fs/cgroup")).unwrap();
        dir
    }

    fn write(root: &Path, relative: &str, contents: &str) {
        fs::write(root.join(relative), contents).unwrap();
    }

    fn meminfo(total: u64, available: u64) -> String {
        format!(
            "MemTotal:       {} kB\nMemAvailable:   {} kB\n",
            total / 1024,
            available / 1024
        )
    }

    #[test]
    fn reclaimable_is_the_page_cache_that_is_not_tmpfs_plus_reclaimable_slab() {
        // file 40 − shmem 31 − mapped 1 = 8 of page cache, + 1 of slab.
        assert_eq!(
            parse_reclaimable(&stat(40 * GIB, 31 * GIB, GIB, 6 * GIB, GIB)),
            Some(9 * GIB)
        );
        // A kernel without `file`/`shmem` is read through `inactive_file`.
        assert_eq!(
            parse_reclaimable("inactive_file 42\nslab_reclaimable 7\n"),
            Some(49)
        );
        assert_eq!(parse_reclaimable("inactive_file 42\n"), Some(42));
        // `file` without `shmem` would count tmpfs as free memory, which is
        // exactly what a host without swap must not do: the fallback is used.
        assert_eq!(parse_reclaimable("file 1000\ninactive_file 42\n"), Some(42));
        assert_eq!(parse_reclaimable("file 1000\nshmem 10\n"), Some(990));
        // cgroup v1's names, and junk, are "no figure" rather than "nothing".
        assert_eq!(
            parse_reclaimable("total_inactive_file 99\ntotal_slab_reclaimable 1\n"),
            None
        );
        assert_eq!(parse_reclaimable(""), None);
        assert_eq!(parse_reclaimable("inactive_file not-a-number\n"), None);
    }

    #[test]
    fn a_reading_takes_the_cache_out_of_the_cgroup_usage() {
        let root = tree("cache");
        write(&root, mem::MEMINFO, &meminfo(48 * GIB, 12 * GIB));
        write(&root, mem::CGROUP_V2_MAX, &format!("{}\n", 48 * GIB));
        write(&root, mem::CGROUP_V2_CURRENT, &format!("{}\n", 46 * GIB));
        write(
            &root,
            CGROUP_V2_STAT,
            &stat(40 * GIB, 31 * GIB, GIB, 6 * GIB, GIB),
        );
        let usage = read(&root).unwrap();
        assert_eq!(usage.total(), 48 * GIB);
        assert_eq!(usage.raw(), 46 * GIB);
        assert_eq!(usage.reclaimable(), 9 * GIB);
        assert_eq!(usage.used(), 37 * GIB);
        assert!(
            usage.percent() < DEFAULT_WATERMARK_PERCENT,
            "{}",
            usage.percent()
        );
        // ... where the raw figure is over the default, which is the case the
        // exclusion exists for.
        assert!(usage.raw() as f64 / usage.total() as f64 * 100.0 > DEFAULT_WATERMARK_PERCENT);
    }

    #[test]
    fn without_a_reclaimable_figure_the_raw_usage_stands() {
        let root = tree("nostat");
        write(&root, mem::MEMINFO, &meminfo(8 * GIB, GIB));
        write(&root, mem::CGROUP_V2_MAX, &format!("{}\n", 8 * GIB));
        write(&root, mem::CGROUP_V2_CURRENT, &format!("{}\n", 6 * GIB));
        let usage = read(&root).unwrap();
        assert_eq!((usage.reclaimable(), usage.used()), (0, 6 * GIB));

        // A `memory.stat` that cannot be parsed does not invent a figure.
        write(&root, CGROUP_V2_STAT, "slab 1\n");
        assert_eq!(read(&root).unwrap().used(), 6 * GIB);
    }

    #[test]
    fn without_cgroups_mem_available_is_already_net_of_the_cache() {
        let root = tree("nocgroup");
        write(&root, mem::MEMINFO, &meminfo(8 * GIB, 2 * GIB));
        // A stale v2 `memory.stat` without `memory.current` is not a reading.
        fs::create_dir_all(root.join("sys/fs/cgroup")).unwrap();
        write(&root, CGROUP_V2_STAT, &stat(6 * GIB, GIB, 0, 6 * GIB, GIB));
        let usage = read(&root).unwrap();
        assert_eq!(usage.total(), 8 * GIB);
        assert_eq!(usage.reclaimable(), 0, "MemAvailable already excludes it");
        assert_eq!(usage.used(), 6 * GIB);
    }

    #[test]
    fn a_boot_refusal_names_the_setting_and_the_reading() {
        let usage = Usage {
            total: 48 * GIB,
            raw: 45 * GIB,
            reclaimable: GIB,
        };
        // 44/48 = 91.7%: over the default, and over a 90% knob too.
        let message = boot_refusal(&usage, 90.0).unwrap();
        assert!(message.contains("BLASTIT_MEM_HIGH_WATERMARK"), "{message}");
        assert!(message.contains("91.7%"), "{message}");
        assert!(
            message.contains("1.0 GiB of page cache and slab left out"),
            "{message}"
        );
        assert!(message.contains("503"), "{message}");
        // At or below the watermark there is no refusal, and one that can read
        // no cache does not claim to have left any out.
        assert!(boot_refusal(&usage, 95.0).is_none());
        let no_cache = Usage {
            total: 48 * GIB,
            raw: 45 * GIB,
            reclaimable: 0,
        };
        let message = boot_refusal(&no_cache, 90.0).unwrap();
        assert!(!message.contains("left out"), "{message}");
    }

    #[test]
    fn the_advisory_is_only_for_a_boot_above_the_default() {
        let above_max = Usage {
            total: 100 * GIB,
            raw: 92 * GIB,
            reclaimable: 2 * GIB,
        };
        // 90%: above the default, below a 95% knob.
        let message = advisory(&above_max, 95.0).unwrap();
        assert!(message.contains("BLASTIT_MEM_HIGH_WATERMARK"), "{message}");
        assert!(message.contains("90.0%"), "{message}");
        assert!(message.contains("2.0 GiB"), "{message}");
        assert!(!message.contains('\n'), "one line: {message}");
        // At the default the boot is normal, and a knob the reading exceeds
        // means the boot refused instead of advising.
        assert!(advisory(&above_max, DEFAULT_WATERMARK_PERCENT).is_none());
        assert!(advisory(&above_max, 85.5).is_none(), "85.5% < 90%: refused");
        let below = Usage {
            total: 100 * GIB,
            raw: 70 * GIB,
            reclaimable: 0,
        };
        assert!(advisory(&below, 100.0).is_none());
    }
}
