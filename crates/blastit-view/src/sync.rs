//! Small locking helpers used across the crate.

use std::sync::{Mutex, MutexGuard, PoisonError};

/// Take a lock, treating a poisoned mutex as usable.
///
/// Nothing held under these mutexes can panic while it is held — a window is
/// updated, a frame is encoded into a `Vec` and written out — so poisoning
/// would only come from a panic in unrelated code, and a view is better off
/// continuing than unwinding into a process that has already lost the GIL.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
