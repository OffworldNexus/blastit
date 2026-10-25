//! Scheduler configuration (ARCHITECTURE.md §2.4, §8.2).
//!
//! Every field mirrors one environment variable, but the scheduler never reads
//! the environment: init parses `BLASTIT_*` in stage 1 and hands the values
//! over (ARCHITECTURE.md §4, §8.2). The defaults here are the documented
//! defaults of those variables.

use std::time::Duration;

use blastit_frame::Limits;

/// How long the scheduler waits before asking init for a view it just lost
/// (§2.4: "don't spin: back off before respawning the same route").
pub const SPAWN_BACKOFF: Duration = Duration::from_secs(1);

/// Everything the scheduler needs to know about its budgets and timers.
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// `BLASTIT_MAX_WORKERS`: live request threads across every view process.
    pub max_workers: u32,
    /// `BLASTIT_VIEW_MAX_WORKERS`: per-view thread cap. `None` means
    /// `BLASTIT_MAX_WORKERS`.
    pub view_max_workers: Option<u32>,
    /// `BLASTIT_QUEUE_SIZE`: requests that may wait for a worker slot.
    pub queue_size: usize,
    /// `BLASTIT_QUEUE_TIMEOUT`: how long one of them may wait.
    pub queue_timeout: Duration,
    /// `BLASTIT_REQUEST_TIMEOUT`: time until the client gets a `504`.
    pub request_timeout: Duration,
    /// `BLASTIT_STUCK_THREADS`: overdue threads before a view is recycled.
    pub stuck_threads: u32,
    /// `BLASTIT_VIEW_SPAWN=eager`: spawn every view at startup.
    pub eager_spawn: bool,
    /// `BLASTIT_VIEW_IDLE_TIMEOUT`: retire an idle view after this long. Zero
    /// disables idle retirement.
    pub idle_timeout: Duration,
    /// `BLASTIT_VIEW_MAX_REQUESTS`: retire a view after this many dispatches.
    /// Zero disables the cap.
    pub max_requests: u64,
    /// `BLASTIT_MAX_VIEW_PROCESSES`: live view process cap, enforced by
    /// retiring the least-recently-used idle view. `None` means no cap.
    pub max_view_processes: Option<u32>,
    /// `BLASTIT_HEALTH_PATH`: answered by the scheduler itself.
    pub health_path: String,
    /// `BLASTIT_SCRIPT_NAME`: mount prefix stripped before routing.
    pub script_name: Option<String>,
    /// `BLASTIT_FRAME_MAX`: the frame limit of every channel the scheduler
    /// speaks on.
    pub frame_limits: Limits,
    /// How long to wait before respawning a route whose last spawn failed.
    pub spawn_backoff: Duration,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_workers: 16,
            view_max_workers: None,
            queue_size: 256,
            queue_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(30),
            stuck_threads: 4,
            eager_spawn: false,
            idle_timeout: Duration::from_secs(600),
            max_requests: 0,
            max_view_processes: None,
            health_path: "/_blastit/health".to_string(),
            script_name: None,
            frame_limits: Limits::default(),
            spawn_backoff: SPAWN_BACKOFF,
        }
    }
}

impl SchedulerConfig {
    /// The per-view thread cap: `BLASTIT_VIEW_MAX_WORKERS`, or the global
    /// budget when that is unset (§8.2).
    pub fn view_worker_cap(&self) -> u32 {
        self.view_max_workers.unwrap_or(self.max_workers)
    }
}
