//! Instrument registry module.
//!
//! This module provides the [`InstrumentRegistry`] for allocating unique numeric
//! instrument IDs and maintaining a reverse index for O(1) lookup by ID.
//!
//! The registry is owned by
//! [`UnderlyingOrderBookManager`](super::underlying::UnderlyingOrderBookManager)
//! and propagated as `Arc<InstrumentRegistry>` through the hierarchy. When a new
//! [`StrikeOrderBook`](super::strike::StrikeOrderBook) is created via the hierarchy,
//! each call/put [`OptionOrderBook`](super::book::OptionOrderBook) is assigned a
//! unique ID from the registry.

use dashmap::DashMap;
use optionstratlib::{ExpirationDate, OptionStyle};
use std::sync::atomic::{AtomicU32, Ordering};

/// Compact instrument metadata stored in the reverse index.
///
/// Contains the coordinates needed to locate an instrument within the
/// order book hierarchy: symbol, expiration, strike, and option style.
#[derive(Debug, Clone)]
pub struct InstrumentInfo {
    /// The full option symbol (e.g., "BTC-20240329-50000-C").
    symbol: String,
    /// The expiration date of the option.
    expiration: ExpirationDate,
    /// The strike price.
    strike: u64,
    /// The option style (Call or Put).
    option_style: OptionStyle,
}

impl InstrumentInfo {
    /// Creates a new instrument info entry.
    ///
    /// # Arguments
    ///
    /// * `symbol` - The full option symbol
    /// * `expiration` - The expiration date
    /// * `strike` - The strike price
    /// * `option_style` - Call or Put
    #[must_use]
    pub fn new(
        symbol: impl Into<String>,
        expiration: ExpirationDate,
        strike: u64,
        option_style: OptionStyle,
    ) -> Self {
        Self {
            symbol: symbol.into(),
            expiration,
            strike,
            option_style,
        }
    }

    /// Returns the full option symbol.
    #[must_use]
    #[inline]
    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    /// Returns the expiration date.
    #[inline]
    pub const fn expiration(&self) -> &ExpirationDate {
        &self.expiration
    }

    /// Returns the strike price.
    #[must_use]
    #[inline]
    pub const fn strike(&self) -> u64 {
        self.strike
    }

    /// Returns the option style (Call or Put).
    #[must_use]
    #[inline]
    pub const fn option_style(&self) -> OptionStyle {
        self.option_style
    }
}

impl std::fmt::Display for InstrumentInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} (strike={}, style={:?})",
            self.symbol, self.strike, self.option_style
        )
    }
}

/// Thread-safe instrument ID allocator and reverse index.
///
/// Provides monotonically increasing `u32` IDs for each
/// [`OptionOrderBook`](super::book::OptionOrderBook) created through the hierarchy,
/// and maintains a `DashMap`-based reverse index for O(1) lookup by ID.
///
/// ## Thread Safety
///
/// The allocator uses [`AtomicU32`] for lock-free ID generation.
/// The reverse index uses [`DashMap`] for concurrent reads and writes via sharded locking.
///
/// ## Seed Support
///
/// Use [`new_with_seed`](Self::new_with_seed) to start IDs from a specific value,
/// allowing IDs to survive hierarchy rebuilds.
pub struct InstrumentRegistry {
    /// The next ID to allocate.
    next_id: AtomicU32,
    /// Reverse index mapping instrument ID → instrument info.
    index: DashMap<u32, InstrumentInfo>,
}

impl InstrumentRegistry {
    /// Creates a new instrument registry with IDs starting from 1.
    ///
    /// ID 0 is reserved for standalone `OptionOrderBook` instances
    /// created outside the hierarchy.
    #[must_use]
    pub fn new() -> Self {
        Self {
            next_id: AtomicU32::new(1),
            index: DashMap::new(),
        }
    }

    /// Creates a new instrument registry with IDs starting from the given seed.
    ///
    /// Use this to resume ID allocation after a hierarchy rebuild.
    /// The seed is clamped to a minimum of 1 since ID 0 is reserved
    /// for standalone books.
    ///
    /// # Arguments
    ///
    /// * `seed` - The starting ID value (clamped to >= 1)
    #[must_use]
    pub fn new_with_seed(seed: u32) -> Self {
        Self {
            next_id: AtomicU32::new(seed.max(1)),
            index: DashMap::new(),
        }
    }

    /// Allocates the next unique instrument ID.
    ///
    /// IDs are monotonically increasing and never reused.
    /// Uses `Ordering::Relaxed` since the only invariant is uniqueness,
    /// not ordering relative to other memory operations.
    ///
    /// # Panics
    ///
    /// Panics if the `u32` counter is exhausted (> 4 billion instruments).
    #[inline]
    pub fn allocate(&self) -> u32 {
        self.next_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .expect("instrument ID counter exhausted")
    }

    /// Registers an instrument in the reverse index.
    ///
    /// # Arguments
    ///
    /// * `id` - The instrument ID (from [`allocate`](Self::allocate))
    /// * `info` - The instrument metadata
    pub fn register(&self, id: u32, info: InstrumentInfo) {
        self.index.insert(id, info);
    }

    /// Looks up instrument info by ID.
    ///
    /// Returns `None` if the ID is not registered.
    ///
    /// # Arguments
    ///
    /// * `id` - The instrument ID to look up
    #[must_use]
    #[inline]
    pub fn get(&self, id: u32) -> Option<InstrumentInfo> {
        self.index.get(&id).map(|entry| entry.value().clone())
    }

    /// Returns the number of registered instruments.
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Returns `true` if no instruments are registered.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Returns the current ID counter value without advancing it.
    ///
    /// This is the next ID that will be allocated. Useful for persisting
    /// the counter state before shutdown.
    #[must_use]
    #[inline]
    pub fn current_id(&self) -> u32 {
        self.next_id.load(Ordering::Relaxed)
    }
}

impl Default for InstrumentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use optionstratlib::prelude::pos_or_panic;

    fn test_expiration() -> ExpirationDate {
        ExpirationDate::Days(pos_or_panic!(30.0))
    }

    #[test]
    fn test_registry_new_starts_from_one() {
        let registry = InstrumentRegistry::new();
        assert_eq!(registry.current_id(), 1);
        assert!(registry.is_empty());
    }

    #[test]
    fn test_registry_new_with_seed() {
        let registry = InstrumentRegistry::new_with_seed(100);
        assert_eq!(registry.current_id(), 100);
    }

    #[test]
    fn test_allocate_monotonic() {
        let registry = InstrumentRegistry::new();
        let id1 = registry.allocate();
        let id2 = registry.allocate();
        let id3 = registry.allocate();

        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(id3, 3);
        assert_eq!(registry.current_id(), 4);
    }

    #[test]
    fn test_allocate_with_seed() {
        let registry = InstrumentRegistry::new_with_seed(50);
        assert_eq!(registry.allocate(), 50);
        assert_eq!(registry.allocate(), 51);
        assert_eq!(registry.current_id(), 52);
    }

    #[test]
    fn test_register_and_get() {
        let registry = InstrumentRegistry::new();
        let id = registry.allocate();

        let info = InstrumentInfo::new(
            "BTC-20240329-50000-C",
            test_expiration(),
            50000,
            OptionStyle::Call,
        );
        registry.register(id, info);

        let retrieved = registry.get(id);
        assert!(retrieved.is_some());

        let retrieved = match retrieved {
            Some(r) => r,
            None => panic!("expected instrument info"),
        };
        assert_eq!(retrieved.symbol(), "BTC-20240329-50000-C");
        assert_eq!(retrieved.strike(), 50000);
        assert_eq!(retrieved.option_style(), OptionStyle::Call);
    }

    #[test]
    fn test_get_missing_returns_none() {
        let registry = InstrumentRegistry::new();
        assert!(registry.get(999).is_none());
    }

    #[test]
    fn test_len_and_is_empty() {
        let registry = InstrumentRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);

        let id = registry.allocate();
        registry.register(
            id,
            InstrumentInfo::new(
                "BTC-20240329-50000-C",
                test_expiration(),
                50000,
                OptionStyle::Call,
            ),
        );

        assert!(!registry.is_empty());
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_multiple_registrations() {
        let registry = InstrumentRegistry::new();

        for i in 0..10 {
            let id = registry.allocate();
            registry.register(
                id,
                InstrumentInfo::new(
                    format!("BTC-20240329-{}-C", 50000 + i * 1000),
                    test_expiration(),
                    50000 + i * 1000,
                    OptionStyle::Call,
                ),
            );
        }

        assert_eq!(registry.len(), 10);
        assert_eq!(registry.current_id(), 11);

        // Verify first and last
        let first = registry.get(1);
        assert!(first.is_some());
        let first = match first {
            Some(f) => f,
            None => panic!("expected instrument info"),
        };
        assert_eq!(first.strike(), 50000);

        let last = registry.get(10);
        assert!(last.is_some());
        let last = match last {
            Some(l) => l,
            None => panic!("expected instrument info"),
        };
        assert_eq!(last.strike(), 59000);
    }

    #[test]
    fn test_concurrent_allocation() {
        use std::sync::Arc;
        use std::thread;

        let registry = Arc::new(InstrumentRegistry::new());
        let mut handles = vec![];

        for _ in 0..4 {
            let reg = Arc::clone(&registry);
            handles.push(thread::spawn(move || {
                let mut ids = Vec::new();
                for _ in 0..100 {
                    ids.push(reg.allocate());
                }
                ids
            }));
        }

        let mut all_ids: Vec<u32> = handles
            .into_iter()
            .flat_map(|h| match h.join() {
                Ok(ids) => ids,
                Err(_) => panic!("thread panicked"),
            })
            .collect();

        all_ids.sort();
        all_ids.dedup();

        // 4 threads × 100 allocations = 400 unique IDs
        assert_eq!(all_ids.len(), 400);
        assert_eq!(registry.current_id(), 401);
    }

    #[test]
    fn test_instrument_info_display() {
        let info = InstrumentInfo::new(
            "BTC-20240329-50000-C",
            test_expiration(),
            50000,
            OptionStyle::Call,
        );
        let display = info.to_string();
        assert!(display.contains("BTC-20240329-50000-C"));
        assert!(display.contains("50000"));
    }

    #[test]
    fn test_instrument_info_clone() {
        let info = InstrumentInfo::new(
            "BTC-20240329-50000-C",
            test_expiration(),
            50000,
            OptionStyle::Call,
        );
        let cloned = info.clone();
        assert_eq!(info.symbol(), cloned.symbol());
        assert_eq!(info.strike(), cloned.strike());
        assert_eq!(info.option_style(), cloned.option_style());
    }

    #[test]
    fn test_default_registry() {
        let registry = InstrumentRegistry::default();
        assert_eq!(registry.current_id(), 1);
        assert!(registry.is_empty());
    }

    #[test]
    fn test_seed_zero_clamped_to_one() {
        let registry = InstrumentRegistry::new_with_seed(0);
        assert_eq!(registry.current_id(), 1);
        assert_eq!(registry.allocate(), 1);
    }
}
