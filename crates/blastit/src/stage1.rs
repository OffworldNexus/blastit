//! Stage 1: the app environment, the front end, the scheduler and the supervisor
//! (§2.1, §4).
//!
//! By the time this stage starts, every secret is gone: the process image was
//! replaced with the app environment (`envp`), dbproxy holds the credentials and
//! init only keeps the read ends of its log pipes. What is left to do is:
//!
//! ```text
//! read the configuration from --config-fd
//! bind 0.0.0.0:$PORT
//! fork http (base+1) and scheduler (base+2) — both before CPython exists
//! initialise CPython, import BLASTIT_APP, preload, warm up, close DB handles
//! gc.collect() · immortalize the preloaded heap (§9.2 #7) · gc.freeze()
//! boot: what the preload cost and the view cap memory buys (§8)
//! ready
//! supervise, holding the GIL
//! ```
//!
//! Nothing here parses HTTP and nothing here talks to the database: init exists
//! to fork, to supervise and to be the only writer to stdout (§2.1).
//!
//! # Where the interpreter comes from
//!
//! This crate never names one. Stage 1 `dlopen`s the pinned per-minor shim (and,
//! for the standalone binary, the host's `libpython`), and from then on every
//! call is a `blastit_py_abi_v1` entry point carried in a
//! [`crate::py::PyRuntime`] (design §1.2.1). [`load`] documents the two orders —
//! `Embed` and `Hosted` — and the forks of `http`/`scheduler` happen before the
//! load in `Embed`, so neither inherits a half-initialised interpreter.

#![cfg_attr(not(feature = "python"), allow(unused_imports))]

use std::os::fd::RawFd;
use std::path::PathBuf;

use blastit_events::{Component, Event, EventKind, Lifecycle};

use crate::config::Config;
use crate::logs::Emitter;

mod adopt;
mod boot;
#[cfg(feature = "python")]
mod load;
mod serve;
mod spawn;

#[cfg(test)]
mod unit_tests;

pub use boot::run;

pub(crate) use adopt::Adoption;

pub use spawn::ViewChildPlans;

/// Where stage 1 finds what stage 0 left it (§4).
#[derive(Debug, Clone)]
pub struct Options {
    /// `--config-fd`: the read end of the configuration pipe.
    pub config_fd: RawFd,
    /// `--dbproxy-pid`: dbproxy's pid, when it was forked.
    pub dbproxy_pid: Option<i32>,
    /// `--dbproxy-events-fd`: dbproxy's events pipe.
    pub dbproxy_events_fd: Option<RawFd>,
    /// `--dbproxy-stdout-fd`: dbproxy's stdout pipe.
    pub dbproxy_stdout_fd: Option<RawFd>,
    /// `--dbproxy-stderr-fd`: dbproxy's stderr pipe.
    pub dbproxy_stderr_fd: Option<RawFd>,
    /// `--dev-root`: the ephemeral runtime root `blastit dev` created (`R20`).
    /// `Some` makes this stage an unprivileged single-uid simulation.
    pub dev_root: Option<PathBuf>,
    /// `--dev-host`: the address the http role binds in `dev`.
    pub dev_host: Option<std::net::IpAddr>,
    /// `--dev-obs-fd`: the read end of dbproxy's `--learn` observation pipe.
    pub dev_obs_fd: Option<RawFd>,
    /// `--dev-ctl-fd`: this stage's end of the `--learn` attribution socket.
    pub dev_ctl_fd: Option<RawFd>,
    /// `--dev-reload=on|off`: whether this boot constructs the live-reload
    /// watcher. Stage 0 forwards the user's `--no-reload` as `off`; absent is
    /// the default, `on`.
    pub dev_reload: bool,
    /// `--dev-adopt-fd`: the read end of a reload cycle's adoption document
    /// (§4.6 step 6). `Some` means this image is the replacement half of a
    /// reload: it binds nothing, forks no `http`/dbproxy, and adopts their pids,
    /// pipes and `http.chan` end from the document.
    pub dev_adopt_fd: Option<RawFd>,
    /// `--dev-policy-fd`: this image's end of the dev-only dbproxy policy
    /// socket (§4.6 step 2). Carried into the next adoption document; the
    /// refresh itself is `MB.23.5.3`'s.
    pub dev_policy_fd: Option<RawFd>,
    /// `--dev-reload-since`: the unix-nanosecond instant a reload cycle began,
    /// for the one-shot `Policy::reconcile` after this image's boot report
    /// (§4.6 step 7).
    pub dev_reload_since: Option<u128>,
    /// `--py-lib-fd`: the pinned `libpython` stage 0 opened (design §2.4).
    #[cfg(feature = "python")]
    pub py_lib_fd: Option<std::os::fd::RawFd>,
    /// `--py-shim-fd`: the pinned per-minor shim, beside `--py-lib-fd`.
    #[cfg(feature = "python")]
    pub py_shim_fd: Option<std::os::fd::RawFd>,
}

/// Emits the `ready` event with the route count and the output variable names
/// (§8.1).
fn emit_ready(emitter: &Emitter, config: &Config, routes: usize) {
    let mut lifecycle = Lifecycle::default()
        .with_stage("serving")
        .with_routes(routes as u32)
        .with_workers(config.max_workers);
    for (proxy, names) in &config.proxy_outputs {
        for name in names {
            lifecycle = lifecycle.with_output(proxy.clone(), name.clone());
        }
    }
    emitter.emit(&Event::now(Component::Init, EventKind::Ready(lifecycle)));
}

/// Cleans up an adopted `dev` runtime root, if any (`R20`), then logs `message`
/// and exits non-zero.
fn fail(emitter: &Emitter, dev: Option<&crate::dev_learn::Dev>, message: &str) -> ! {
    if let Some(dev) = dev {
        dev.cleanup();
    }
    crate::logs::fail(emitter, message)
}
