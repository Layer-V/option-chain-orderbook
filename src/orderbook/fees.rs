//! Fee schedule configuration module.
//!
//! This module provides [`SharedFeeSchedule`] for propagating fee configuration
//! through the option chain hierarchy. When a [`FeeSchedule`] is configured,
//! trade results include maker and taker fee calculations.
//!
//! The underlying `OrderBook<T>` supports configurable fee schedules via
//! [`FeeSchedule`]:
//! - **None** (default) — no fees applied
//! - **Maker/taker fees** — specified in basis points (bps)
//! - **Maker rebates** — negative maker bps provide rebates

use orderbook_rs::FeeSchedule;
use std::sync::RwLock;

/// Thread-safe shared fee schedule configuration.
///
/// Wraps an [`Option<FeeSchedule>`] in a [`RwLock`] so that hierarchy managers
/// can store and update the fee schedule for future children without
/// requiring `&mut self`. Follows the same pattern as
/// [`SharedSTPMode`](super::stp::SharedSTPMode).
pub(crate) struct SharedFeeSchedule {
    /// The inner fee schedule, protected by a read-write lock.
    inner: RwLock<Option<FeeSchedule>>,
}

impl SharedFeeSchedule {
    /// Creates a new shared fee schedule defaulting to `None` (no fees).
    #[inline]
    pub(crate) fn new() -> Self {
        Self {
            inner: RwLock::new(None),
        }
    }

    /// Sets the fee schedule.
    ///
    /// Pass `Some(schedule)` to enable fees, or `None` to disable.
    /// Recovers from a poisoned lock to ensure the schedule is always written.
    pub(crate) fn set(&self, schedule: Option<FeeSchedule>) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = schedule;
    }

    /// Returns the current fee schedule, or `None` if no fees are configured.
    ///
    /// Recovers from a poisoned lock to avoid silently dropping a stored schedule.
    #[must_use]
    pub(crate) fn get(&self) -> Option<FeeSchedule> {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard
    }
}

impl Default for SharedFeeSchedule {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SharedFeeSchedule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let schedule = self.get();
        f.debug_struct("SharedFeeSchedule")
            .field("inner", &schedule)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shared_fee_schedule_default_is_none() {
        let shared = SharedFeeSchedule::new();
        assert!(shared.get().is_none());
    }

    #[test]
    fn test_shared_fee_schedule_set_get() {
        let shared = SharedFeeSchedule::new();
        let schedule = FeeSchedule::new(-2, 5);
        shared.set(Some(schedule));
        let result = shared.get();
        assert!(result.is_some());
        let s = match result {
            Some(s) => s,
            None => panic!("expected fee schedule"),
        };
        assert_eq!(s.maker_fee_bps, -2);
        assert_eq!(s.taker_fee_bps, 5);
    }

    #[test]
    fn test_shared_fee_schedule_overwrite() {
        let shared = SharedFeeSchedule::new();
        shared.set(Some(FeeSchedule::new(-2, 5)));
        shared.set(Some(FeeSchedule::new(-5, 10)));
        let s = match shared.get() {
            Some(s) => s,
            None => panic!("expected fee schedule"),
        };
        assert_eq!(s.maker_fee_bps, -5);
        assert_eq!(s.taker_fee_bps, 10);
    }

    #[test]
    fn test_shared_fee_schedule_clear() {
        let shared = SharedFeeSchedule::new();
        shared.set(Some(FeeSchedule::new(-2, 5)));
        shared.set(None);
        assert!(shared.get().is_none());
    }

    #[test]
    fn test_shared_fee_schedule_zero_fee() {
        let shared = SharedFeeSchedule::new();
        shared.set(Some(FeeSchedule::zero_fee()));
        let s = match shared.get() {
            Some(s) => s,
            None => panic!("expected fee schedule"),
        };
        assert!(s.is_zero_fee());
    }

    #[test]
    fn test_shared_fee_schedule_debug() {
        let shared = SharedFeeSchedule::new();
        let debug = format!("{shared:?}");
        assert!(debug.contains("SharedFeeSchedule"));
    }

    #[test]
    fn test_shared_fee_schedule_default_trait() {
        let shared = SharedFeeSchedule::default();
        assert!(shared.get().is_none());
    }
}
