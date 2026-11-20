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
//!
//! # One reader, one reading
//!
//! [`blastit_sys::mem::Reader`] owns the raw sample and every file format:
//! `MemTotal`, both cgroup layouts, and the `total`/`used` figures
//! [`blastit_sys::mem::Sample`] exposes. This module owns the *refined* reading
//! blastit acts on — that sample plus the `memory.stat` bytes the kernel can
//! reclaim — and adds no second parser of those files. [`sample`] is the whole
//! layering: one `Reader::sample`, then `reclaimable`.

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
/// The raw figures are [`blastit_sys::mem::Reader::sample`]'s — there is no
/// second parser of `/proc/meminfo` or the cgroup files — and this adds only the
/// `memory.stat` subtraction.
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
