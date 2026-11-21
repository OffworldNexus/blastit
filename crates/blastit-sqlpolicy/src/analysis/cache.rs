//! The analysis cache, and the two free functions a manifest tool calls.
//!
//! Parsing is expensive and an app runs the same statements over and over, so
//! analyses are cached by the statement's **normalised** text (§6): the shape the
//! policy compares, not the exact bytes. The cache is bounded
//! ([`CACHE_CAPACITY`] entries, least recently used first), because its keys come
//! from an untrusted process.
//!
//! # One owner, injected
//!
//! [`AnalysisCache`] is the owner: a cheap handle to one bounded, shared memo.
//! [`crate::Allowlist`] holds one and checks every statement against *that*
//! cache, so the policy path never reaches for process-wide state of its own.
//!
//! [`analyze`] and [`cached_analyses`] are the crate's convenience API for the
//! manifest tooling (`blastit-manifest`, the Python audit), which analyses
//! standalone statements with no `Allowlist` to hang a cache on. They are the one
//! place that needs a process-wide memo, so [`AnalysisCache::shared`] holds it —
//! this is the only such item in the crate, and it exists so that the frozen
//! cache contract (`analyze` populates what `cached_analyses` reports) keeps
//! holding.

use std::sync::{Arc, Mutex, OnceLock};

use crate::lru::Lru;

use super::types::{Analysis, AnalysisError};
use super::walk::resolve;

/// How many analyses one process keeps.
///
/// A real route sends on the order of a hundred distinct statements (a Django
/// admin can send a few hundred); the cache is a per-process memo, and 512
/// entries of a few hundred bytes each is well under a megabyte.
pub(crate) const CACHE_CAPACITY: usize = 512;

/// One statement's analysis, shared so the cache can hand it out without
/// cloning it.
type Cached = Arc<Result<Analysis, AnalysisError>>;

/// A bounded memo of analyses, keyed by the statement's normalised text.
///
/// Cloning the handle shares the memo, which is what lets a route's allowlist and
/// the tooling look at the same entries without a global. [`AnalysisCache::private`]
/// builds one of its own; [`AnalysisCache::shared`] is the process-wide one the
/// free functions use.
#[derive(Clone, Debug)]
pub(crate) struct AnalysisCache {
    entries: Arc<Mutex<Lru<String, Cached>>>,
}

impl Default for AnalysisCache {
    /// The process-wide memo, which is what every [`crate::Allowlist`] built by
    /// [`crate::Allowlist::new`] or `Default` uses: the policy keeps the
    /// per-process memo it always had, and receives it through its constructor
    /// rather than reading a global.
    fn default() -> Self {
        Self::shared()
    }
}

impl AnalysisCache {
    /// An empty cache of its own, bounded by [`CACHE_CAPACITY`].
    ///
    /// A caller that wants an isolated memo — a test that watches one route's
    /// entries, or a short-lived checker — builds one of these instead of
    /// sharing the process-wide one.
    pub(crate) fn private() -> Self {
        Self {
            entries: Arc::new(Mutex::new(Lru::new(CACHE_CAPACITY))),
        }
    }

    /// The one process-wide memo, built on first use.
    ///
    /// Only the manifest-only free functions ([`analyze`], [`cached_analyses`])
    /// and the default [`crate::Allowlist`] reach for this; every other caller
    /// passes an [`AnalysisCache`] it owns.
    pub(crate) fn shared() -> Self {
        static SHARED: OnceLock<AnalysisCache> = OnceLock::new();
        SHARED.get_or_init(Self::private).clone()
    }

    /// [`analyze`], cached by the statement's normalised text.
    ///
    /// A statement that fails to analyse is cached too — reaching a refusal costs
    /// a parse as well, and a view that keeps sending it should not cost one
    /// every time.
    ///
    /// **`sql` is the text that is parsed**, so it is the statement as PostgreSQL
    /// receives it, with `$n` placeholders: the normalised form (`$?`) is not
    /// valid SQL and would be refused. `normalized` is only the key, which is why
    /// two texts that normalise alike — one `IN ($1)` and one `IN ($1, $2, $3)` —
    /// share an entry, and why the *first* of them decides what the others get:
    /// the shape is what the accesses depend on (§6, D7). Every caller inside
    /// blastit hands over the wire text, so this only matters to a caller of the
    /// public [`analyze`].
    pub(crate) fn analyze_cached(&self, sql: &str, normalized: &str) -> Cached {
        if let Some(hit) = self
            .entries
            .lock()
            .ok()
            .and_then(|mut cache| cache.get(&normalized.to_string()).cloned())
        {
            return hit;
        }
        let analysis = Arc::new(resolve(sql));
        if let Ok(mut cache) = self.entries.lock() {
            cache.insert(normalized.to_string(), Arc::clone(&analysis));
        }
        analysis
    }

    /// How many analyses this cache is currently holding.
    pub(crate) fn len(&self) -> usize {
        self.entries
            .lock()
            .map(|cache| cache.len())
            .unwrap_or_default()
    }
}

/// Analyses one statement (§6).
///
/// Returns what the statement reads and writes, table by table and column by
/// column, plus the functions it calls — or why it cannot be analysed.
///
/// This is the manifest-only convenience: it uses the process-wide memo
/// (`AnalysisCache::shared`). A policy check goes through
/// [`crate::Allowlist::check`], which analyses against the cache its allowlist
/// owns.
///
/// # Errors
///
/// [`AnalysisError`] when the text is not exactly one statement of a kind this
/// build analyses, or when it uses a construct the analysis refuses. Nothing is
/// guessed at: a statement this function refuses is one a table grant can never
/// allow.
pub fn analyze(sql: &str) -> Result<Analysis, AnalysisError> {
    AnalysisCache::shared()
        .analyze_cached(sql, &crate::normalize(sql))
        .as_ref()
        .clone()
}

/// How many analyses the process is currently holding.
///
/// This is the size of the process-wide cache (`AnalysisCache::shared`), not
/// of one route's, and it exists so that a test can see the bound hold.
pub fn cached_analyses() -> usize {
    AnalysisCache::shared().len()
}
