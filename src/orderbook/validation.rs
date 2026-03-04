//! Order validation configuration module.
//!
//! This module provides the [`ValidationConfig`] struct for configuring
//! pre-trade validation rules (tick size, lot size, min/max order size)
//! across the option chain hierarchy.

use std::sync::RwLock;

/// Configuration for order validation rules.
///
/// Controls pre-trade validation at the `OrderBook` level:
/// - **Tick size**: prices must be exact multiples of the tick size
/// - **Lot size**: quantities must be exact multiples of the lot size
/// - **Min order size**: orders below this quantity are rejected
/// - **Max order size**: orders above this quantity are rejected
///
/// All fields default to `None`, which disables the corresponding validation.
///
/// # Examples
///
/// ```
/// use option_chain_orderbook::orderbook::ValidationConfig;
///
/// let config = ValidationConfig::new()
///     .with_tick_size(100)
///     .with_lot_size(10)
///     .with_min_order_size(1)
///     .with_max_order_size(1_000_000);
///
/// assert_eq!(config.tick_size(), Some(100));
/// assert_eq!(config.lot_size(), Some(10));
/// assert!(!config.is_empty());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidationConfig {
    /// Minimum price increment. When set, order prices must be exact multiples
    /// of this value. `None` disables tick size validation.
    tick_size: Option<u128>,
    /// Minimum quantity increment. When set, order quantities must be exact
    /// multiples of this value. `None` disables lot size validation.
    lot_size: Option<u64>,
    /// Minimum allowed order quantity. Orders with quantity below this value
    /// are rejected. `None` disables minimum size validation.
    min_order_size: Option<u64>,
    /// Maximum allowed order quantity. Orders with quantity above this value
    /// are rejected. `None` disables maximum size validation.
    max_order_size: Option<u64>,
}

impl ValidationConfig {
    /// Creates a new empty validation configuration with all rules disabled.
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the tick size (minimum price increment).
    ///
    /// # Arguments
    ///
    /// * `tick_size` - Minimum price increment in smallest price units
    #[must_use]
    #[inline]
    pub const fn with_tick_size(mut self, tick_size: u128) -> Self {
        self.tick_size = Some(tick_size);
        self
    }

    /// Sets the lot size (minimum quantity increment).
    ///
    /// # Arguments
    ///
    /// * `lot_size` - Minimum quantity increment in smallest quantity units
    #[must_use]
    #[inline]
    pub const fn with_lot_size(mut self, lot_size: u64) -> Self {
        self.lot_size = Some(lot_size);
        self
    }

    /// Sets the minimum order size.
    ///
    /// # Arguments
    ///
    /// * `min` - Minimum allowed order quantity
    #[must_use]
    #[inline]
    pub const fn with_min_order_size(mut self, min: u64) -> Self {
        self.min_order_size = Some(min);
        self
    }

    /// Sets the maximum order size.
    ///
    /// # Arguments
    ///
    /// * `max` - Maximum allowed order quantity
    #[must_use]
    #[inline]
    pub const fn with_max_order_size(mut self, max: u64) -> Self {
        self.max_order_size = Some(max);
        self
    }

    /// Returns the configured tick size, if any.
    #[must_use]
    #[inline]
    pub const fn tick_size(&self) -> Option<u128> {
        self.tick_size
    }

    /// Returns the configured lot size, if any.
    #[must_use]
    #[inline]
    pub const fn lot_size(&self) -> Option<u64> {
        self.lot_size
    }

    /// Returns the configured minimum order size, if any.
    #[must_use]
    #[inline]
    pub const fn min_order_size(&self) -> Option<u64> {
        self.min_order_size
    }

    /// Returns the configured maximum order size, if any.
    #[must_use]
    #[inline]
    pub const fn max_order_size(&self) -> Option<u64> {
        self.max_order_size
    }

    /// Returns `true` if no validation rules are configured.
    #[must_use]
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.tick_size.is_none()
            && self.lot_size.is_none()
            && self.min_order_size.is_none()
            && self.max_order_size.is_none()
    }
}

impl std::fmt::Display for ValidationConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_empty() {
            return write!(f, "ValidationConfig(none)");
        }
        write!(f, "ValidationConfig(")?;
        let mut first = true;
        if let Some(tick) = self.tick_size {
            write!(f, "tick={tick}")?;
            first = false;
        }
        if let Some(lot) = self.lot_size {
            if !first {
                write!(f, ", ")?;
            }
            write!(f, "lot={lot}")?;
            first = false;
        }
        if let Some(min) = self.min_order_size {
            if !first {
                write!(f, ", ")?;
            }
            write!(f, "min={min}")?;
            first = false;
        }
        if let Some(max) = self.max_order_size {
            if !first {
                write!(f, ", ")?;
            }
            write!(f, "max={max}")?;
        }
        write!(f, ")")
    }
}

/// Thread-safe shared validation configuration.
///
/// Wraps a [`ValidationConfig`] in a [`RwLock`] so that hierarchy managers
/// can store and update the validation config for future children without
/// requiring `&mut self`.
pub(crate) struct SharedValidationConfig {
    /// The inner validation config, protected by a read-write lock.
    inner: RwLock<Option<ValidationConfig>>,
}

impl SharedValidationConfig {
    /// Creates a new empty shared validation config.
    #[inline]
    pub(crate) fn new() -> Self {
        Self {
            inner: RwLock::new(None),
        }
    }

    /// Sets the validation config.
    ///
    /// Recovers from a poisoned lock to ensure the config is always written.
    pub(crate) fn set(&self, config: ValidationConfig) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = Some(config);
    }

    /// Returns a clone of the current validation config, if any.
    ///
    /// Recovers from a poisoned lock to avoid silently dropping a stored config.
    #[must_use]
    pub(crate) fn get(&self) -> Option<ValidationConfig> {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.clone()
    }
}

impl Default for SharedValidationConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SharedValidationConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let config = self.get();
        f.debug_struct("SharedValidationConfig")
            .field("inner", &config)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_config_default_is_empty() {
        let config = ValidationConfig::new();
        assert!(config.is_empty());
        assert_eq!(config.tick_size(), None);
        assert_eq!(config.lot_size(), None);
        assert_eq!(config.min_order_size(), None);
        assert_eq!(config.max_order_size(), None);
    }

    #[test]
    fn test_validation_config_builder() {
        let config = ValidationConfig::new()
            .with_tick_size(100)
            .with_lot_size(10)
            .with_min_order_size(1)
            .with_max_order_size(1_000_000);

        assert!(!config.is_empty());
        assert_eq!(config.tick_size(), Some(100));
        assert_eq!(config.lot_size(), Some(10));
        assert_eq!(config.min_order_size(), Some(1));
        assert_eq!(config.max_order_size(), Some(1_000_000));
    }

    #[test]
    fn test_validation_config_partial() {
        let config = ValidationConfig::new().with_tick_size(50);

        assert!(!config.is_empty());
        assert_eq!(config.tick_size(), Some(50));
        assert_eq!(config.lot_size(), None);
    }

    #[test]
    fn test_validation_config_clone() {
        let config = ValidationConfig::new()
            .with_tick_size(100)
            .with_lot_size(10);
        let cloned = config.clone();
        assert_eq!(config, cloned);
    }

    #[test]
    fn test_validation_config_display_empty() {
        let config = ValidationConfig::new();
        assert_eq!(format!("{config}"), "ValidationConfig(none)");
    }

    #[test]
    fn test_validation_config_display_full() {
        let config = ValidationConfig::new()
            .with_tick_size(100)
            .with_lot_size(10)
            .with_min_order_size(1)
            .with_max_order_size(500);
        let display = format!("{config}");
        assert!(display.contains("tick=100"));
        assert!(display.contains("lot=10"));
        assert!(display.contains("min=1"));
        assert!(display.contains("max=500"));
    }

    #[test]
    fn test_validation_config_display_partial() {
        let config = ValidationConfig::new().with_lot_size(5);
        assert_eq!(format!("{config}"), "ValidationConfig(lot=5)");
    }

    #[test]
    fn test_shared_validation_config_default() {
        let shared = SharedValidationConfig::new();
        assert!(shared.get().is_none());
    }

    #[test]
    fn test_shared_validation_config_set_get() {
        let shared = SharedValidationConfig::new();
        let config = ValidationConfig::new().with_tick_size(100);
        shared.set(config.clone());
        assert_eq!(shared.get(), Some(config));
    }

    #[test]
    fn test_shared_validation_config_overwrite() {
        let shared = SharedValidationConfig::new();
        shared.set(ValidationConfig::new().with_tick_size(100));
        shared.set(ValidationConfig::new().with_tick_size(200));
        assert_eq!(shared.get().map(|c| c.tick_size()), Some(Some(200)));
    }

    #[test]
    fn test_shared_validation_config_debug() {
        let shared = SharedValidationConfig::new();
        let debug = format!("{shared:?}");
        assert!(debug.contains("SharedValidationConfig"));
    }
}
