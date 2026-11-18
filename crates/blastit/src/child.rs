//! The child sequence every forked process runs — **now in `blastit-sys`**
//! (§3, §4, §7.2, `view-crates-architecture` §4.4 D1).
//!
//! [`blastit_sys::child`] owns the implementation: [`ChildIo`], the view child
//! sequence [`enter`], the steps it composes, and the small helpers a child
//! needs (`child_exit`, the `poll` waits, the deadline, `panic_message`,
//! `set_env`). It lives there because the view forks are moving out of this
//! crate: the WSGI one is `blastit-view-python`'s today, and the two PHP ones
//! (`blastit-view-php`'s `spawn` and `probe`) follow in `MB.36.5`. A view crate
//! may not depend on `blastit`, so the sequence they share cannot stay here.
//!
//! This module stays so that every `crate::child::…` call site in `stage0`,
//! `stage1`, `supervise`, `visibility` and `php_view` keeps compiling
//! unchanged: one re-export, no second spelling of any step.
//!
//! [`ChildIo`]: blastit_sys::child::ChildIo
//! [`enter`]: blastit_sys::child::enter

pub(crate) use blastit_sys::child::*;
