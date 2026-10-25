//! A small bounded cache: least recently used, one entry per key.
//!
//! [`crate::analysis`] parses a statement with PostgreSQL's own parser, which is
//! far too expensive to repeat for a statement an app runs on every request, and
//! far too expensive to let a memory-bound process grow without bound. The keys
//! come from outside (a statement's normalised text), so the cache has to be
//! bounded by *entries*, never by what a caller asks for.

use std::collections::HashMap;
use std::hash::Hash;

/// A bounded map that forgets the least recently used entry first.
///
/// `get` counts as a use, so a statement that keeps arriving keeps its entry;
/// `insert` on a full cache evicts the entry used longest ago. Eviction is a
/// linear scan over the map, which is the right trade for the small capacities
/// this is used with (`crate::analysis::CACHE_CAPACITY`).
#[derive(Debug)]
pub(crate) struct Lru<K, V> {
    entries: HashMap<K, (V, u64)>,
    /// A monotonically increasing use counter: the *lowest* stamp is the least
    /// recently used entry.
    clock: u64,
    capacity: usize,
}

impl<K: Eq + Hash + Clone, V> Lru<K, V> {
    /// An empty cache holding at most `capacity` entries.
    ///
    /// A capacity of zero is allowed and makes the cache forget everything at
    /// once: it is then a pass-through, not an error.
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            clock: 0,
            capacity,
        }
    }

    /// The value of `key`, marking it as the most recently used.
    pub(crate) fn get(&mut self, key: &K) -> Option<&V> {
        self.clock = self.clock.wrapping_add(1);
        let clock = self.clock;
        let (value, stamp) = self.entries.get_mut(key)?;
        *stamp = clock;
        Some(value)
    }

    /// Stores `value` under `key`, evicting the least recently used entry when
    /// the cache is full.
    pub(crate) fn insert(&mut self, key: K, value: V) {
        if self.capacity == 0 {
            return;
        }
        self.clock = self.clock.wrapping_add(1);
        if self.len() >= self.capacity
            && !self.entries.contains_key(&key)
            && let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, (_, stamp))| *stamp)
                .map(|(key, _)| key.clone())
        {
            self.entries.remove(&oldest);
        }
        let clock = self.clock;
        self.entries.insert(key, (value, clock));
    }

    /// How many entries the cache holds.
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_cache_evicts_the_least_recently_used_entry() {
        let mut cache: Lru<u32, u32> = Lru::new(2);
        cache.insert(1, 10);
        cache.insert(2, 20);
        // Touching 1 makes 2 the least recently used one.
        assert_eq!(cache.get(&1), Some(&10));
        cache.insert(3, 30);
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get(&2), None, "2 was the least recently used");
        assert_eq!(cache.get(&1), Some(&10));
        assert_eq!(cache.get(&3), Some(&30));
    }

    #[test]
    fn the_cache_never_grows_past_its_capacity() {
        let mut cache: Lru<u32, u32> = Lru::new(4);
        for key in 0..1_000 {
            cache.insert(key, key);
            assert!(cache.len() <= 4, "the cache grew to {}", cache.len());
        }
        assert_eq!(cache.len(), 4);
    }

    #[test]
    fn reinserting_a_key_does_not_evict_anything() {
        let mut cache: Lru<u32, u32> = Lru::new(2);
        cache.insert(1, 10);
        cache.insert(2, 20);
        cache.insert(1, 11);
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get(&1), Some(&11));
        assert_eq!(cache.get(&2), Some(&20));
    }

    #[test]
    fn a_zero_capacity_cache_stores_nothing() {
        let mut cache: Lru<u32, u32> = Lru::new(0);
        cache.insert(1, 10);
        assert_eq!(cache.len(), 0);
        assert_eq!(cache.get(&1), None);
    }

    #[test]
    fn a_key_a_caller_asks_for_is_still_found_after_a_clock_wrap() {
        // The clock wraps rather than panicking; with two entries it cannot
        // confuse them, because they are never more than 2^64 ticks apart.
        let mut cache: Lru<u32, u32> = Lru::new(2);
        cache.clock = u64::MAX;
        cache.insert(1, 10);
        cache.insert(2, 20);
        assert_eq!(cache.get(&1), Some(&10));
        cache.insert(3, 30);
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get(&2), None);
    }
}
