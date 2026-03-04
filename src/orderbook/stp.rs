//! Self-trade prevention (STP) configuration module.
//!
//! This module provides [`SharedSTPMode`] for propagating STP configuration
//! through the option chain hierarchy. STP prevents a trader's incoming order
//! from matching against their own resting orders.
//!
//! The underlying `OrderBook<T>` supports three STP modes via
//! [`STPMode`]:
//! - **None** (default) — no self-trade prevention
//! - **CancelTaker** — cancel the incoming order on self-trade
//! - **CancelMaker** — cancel the resting order on self-trade
//! - **CancelBoth** — cancel both orders on self-trade

use orderbook_rs::STPMode;
use std::sync::RwLock;

/// Thread-safe shared STP mode configuration.
///
/// Wraps an [`STPMode`] in a [`RwLock`] so that hierarchy managers
/// can store and update the STP mode for future children without
/// requiring `&mut self`. Follows the same pattern as
/// [`SharedValidationConfig`](super::validation::SharedValidationConfig).
pub(crate) struct SharedSTPMode {
    /// The inner STP mode, protected by a read-write lock.
    inner: RwLock<STPMode>,
}

impl SharedSTPMode {
    /// Creates a new shared STP mode defaulting to [`STPMode::None`].
    #[inline]
    pub(crate) fn new() -> Self {
        Self {
            inner: RwLock::new(STPMode::None),
        }
    }

    /// Sets the STP mode.
    ///
    /// Recovers from a poisoned lock to ensure the mode is always written.
    pub(crate) fn set(&self, mode: STPMode) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = mode;
    }

    /// Returns the current STP mode.
    ///
    /// Recovers from a poisoned lock to avoid silently dropping a stored mode.
    #[must_use]
    pub(crate) fn get(&self) -> STPMode {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard
    }
}

impl Default for SharedSTPMode {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SharedSTPMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mode = self.get();
        f.debug_struct("SharedSTPMode")
            .field("inner", &mode)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shared_stp_mode_default_is_none() {
        let shared = SharedSTPMode::new();
        assert_eq!(shared.get(), STPMode::None);
    }

    #[test]
    fn test_shared_stp_mode_set_get() {
        let shared = SharedSTPMode::new();
        shared.set(STPMode::CancelTaker);
        assert_eq!(shared.get(), STPMode::CancelTaker);
    }

    #[test]
    fn test_shared_stp_mode_overwrite() {
        let shared = SharedSTPMode::new();
        shared.set(STPMode::CancelTaker);
        shared.set(STPMode::CancelBoth);
        assert_eq!(shared.get(), STPMode::CancelBoth);
    }

    #[test]
    fn test_shared_stp_mode_cancel_maker() {
        let shared = SharedSTPMode::new();
        shared.set(STPMode::CancelMaker);
        assert_eq!(shared.get(), STPMode::CancelMaker);
    }

    #[test]
    fn test_shared_stp_mode_debug() {
        let shared = SharedSTPMode::new();
        let debug = format!("{shared:?}");
        assert!(debug.contains("SharedSTPMode"));
    }

    #[test]
    fn test_shared_stp_mode_default_trait() {
        let shared = SharedSTPMode::default();
        assert_eq!(shared.get(), STPMode::None);
    }
}
