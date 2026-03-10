//! Mark price calculation module.
//!
//! This module provides [`MarkPriceCalculator`] for computing the mark price as a
//! weighted average of index price, order book mid price, and last trade price,
//! with configurable dampening for manipulation resistance.
//!
//! ## Overview
//!
//! Mark price is used for:
//! - Position valuation and P&L calculation
//! - Margin requirement computation
//! - Liquidation triggering
//!
//! The calculator combines three price sources with configurable weights:
//! - **Index price**: External reference price (e.g., from Chainlink)
//! - **Mid price**: Order book best bid/ask midpoint
//! - **Last trade price**: Most recent execution price
//!
//! ## Dampening
//!
//! To prevent manipulation, the mark price change is limited per update by the
//! dampening factor. For example, with `dampening_factor = 0.01`, the mark price
//! can only move ±1% from its previous value in a single update.
//!
//! ## Example
//!
//! ```
//! use option_chain_orderbook::orderbook::{MarkPriceCalculator, MarkPriceConfig};
//!
//! let config = MarkPriceConfig::builder()
//!     .index_weight(0.5)
//!     .mid_weight(0.3)
//!     .last_trade_weight(0.2)
//!     .dampening_factor(0.01)
//!     .build()
//!     .expect("valid config");
//!
//! let calculator = MarkPriceCalculator::new(config);
//!
//! calculator.update_index_price(50000);
//! calculator.update_mid_price(50100);
//! calculator.update_last_trade_price(50050);
//!
//! let mark = calculator.mark_price();
//! assert!(mark.is_some());
//! ```

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

/// Configuration for mark price calculation.
///
/// Defines the weights for each price source and the dampening factor that
/// limits how much the mark price can change per update.
///
/// ## Validation
///
/// - All weights must be in the range [0.0, 1.0]
/// - Weights must sum to 1.0 (within a small internal tolerance)
/// - Dampening factor must be in the range (0.0, 1.0]
///
/// ## Example
///
/// ```
/// use option_chain_orderbook::orderbook::MarkPriceConfig;
///
/// let config = MarkPriceConfig::builder()
///     .index_weight(0.5)
///     .mid_weight(0.3)
///     .last_trade_weight(0.2)
///     .build()
///     .expect("valid config");
///
/// assert_eq!(config.index_weight(), 0.5);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkPriceConfig {
    /// Weight for index price in range [0.0, 1.0].
    index_weight: f64,
    /// Weight for order book mid price in range [0.0, 1.0].
    mid_weight: f64,
    /// Weight for last trade price in range [0.0, 1.0].
    last_trade_weight: f64,
    /// Maximum price change per update as a fraction in range (0.0, 1.0].
    /// For example, 0.01 means mark price can move at most 1% per update.
    dampening_factor: f64,
}

impl Default for MarkPriceConfig {
    fn default() -> Self {
        Self {
            index_weight: 0.5,
            mid_weight: 0.3,
            last_trade_weight: 0.2,
            dampening_factor: 0.01,
        }
    }
}

impl MarkPriceConfig {
    /// Creates a new builder for `MarkPriceConfig`.
    #[must_use]
    pub fn builder() -> MarkPriceConfigBuilder {
        MarkPriceConfigBuilder::new()
    }

    /// Returns the weight for index price.
    #[must_use]
    #[inline]
    pub fn index_weight(&self) -> f64 {
        self.index_weight
    }

    /// Returns the weight for mid price.
    #[must_use]
    #[inline]
    pub fn mid_weight(&self) -> f64 {
        self.mid_weight
    }

    /// Returns the weight for last trade price.
    #[must_use]
    #[inline]
    pub fn last_trade_weight(&self) -> f64 {
        self.last_trade_weight
    }

    /// Returns the dampening factor.
    #[must_use]
    #[inline]
    pub fn dampening_factor(&self) -> f64 {
        self.dampening_factor
    }

    /// Validates the configuration.
    ///
    /// # Errors
    ///
    /// Returns `Error::ConfigurationError` if:
    /// - Any weight is outside [0.0, 1.0]
    /// - Weights don't sum to approximately 1.0
    /// - Dampening factor is outside (0.0, 1.0]
    pub fn validate(&self) -> Result<()> {
        // Check weight bounds (reject NaN/Infinity)
        if !self.index_weight.is_finite() || !(0.0..=1.0).contains(&self.index_weight) {
            return Err(Error::configuration(format!(
                "index_weight must be a finite value in [0.0, 1.0], got {}",
                self.index_weight
            )));
        }
        if !self.mid_weight.is_finite() || !(0.0..=1.0).contains(&self.mid_weight) {
            return Err(Error::configuration(format!(
                "mid_weight must be a finite value in [0.0, 1.0], got {}",
                self.mid_weight
            )));
        }
        if !self.last_trade_weight.is_finite() || !(0.0..=1.0).contains(&self.last_trade_weight) {
            return Err(Error::configuration(format!(
                "last_trade_weight must be a finite value in [0.0, 1.0], got {}",
                self.last_trade_weight
            )));
        }

        // Check weights sum to 1.0 (with tolerance for floating point)
        let sum = self.index_weight + self.mid_weight + self.last_trade_weight;
        if (sum - 1.0).abs() > 0.001 {
            return Err(Error::configuration(format!(
                "weights must sum to 1.0, got {}",
                sum
            )));
        }

        // Check dampening factor (reject NaN/Infinity)
        if !self.dampening_factor.is_finite()
            || self.dampening_factor <= 0.0
            || self.dampening_factor > 1.0
        {
            return Err(Error::configuration(format!(
                "dampening_factor must be a finite value in (0.0, 1.0], got {}",
                self.dampening_factor
            )));
        }

        Ok(())
    }
}

/// Builder for [`MarkPriceConfig`].
///
/// Provides a fluent interface for constructing mark price configuration
/// with validation on build.
///
/// ## Example
///
/// ```
/// use option_chain_orderbook::orderbook::MarkPriceConfig;
///
/// let config = MarkPriceConfig::builder()
///     .index_weight(0.6)
///     .mid_weight(0.25)
///     .last_trade_weight(0.15)
///     .dampening_factor(0.02)
///     .build()
///     .expect("valid config");
/// ```
#[derive(Debug, Clone, Default)]
pub struct MarkPriceConfigBuilder {
    index_weight: Option<f64>,
    mid_weight: Option<f64>,
    last_trade_weight: Option<f64>,
    dampening_factor: Option<f64>,
}

impl MarkPriceConfigBuilder {
    /// Creates a new builder with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the weight for index price.
    ///
    /// # Arguments
    ///
    /// * `weight` - Weight in range [0.0, 1.0]
    #[must_use]
    pub fn index_weight(mut self, weight: f64) -> Self {
        self.index_weight = Some(weight);
        self
    }

    /// Sets the weight for mid price.
    ///
    /// # Arguments
    ///
    /// * `weight` - Weight in range [0.0, 1.0]
    #[must_use]
    pub fn mid_weight(mut self, weight: f64) -> Self {
        self.mid_weight = Some(weight);
        self
    }

    /// Sets the weight for last trade price.
    ///
    /// # Arguments
    ///
    /// * `weight` - Weight in range [0.0, 1.0]
    #[must_use]
    pub fn last_trade_weight(mut self, weight: f64) -> Self {
        self.last_trade_weight = Some(weight);
        self
    }

    /// Sets the dampening factor.
    ///
    /// # Arguments
    ///
    /// * `factor` - Maximum price change per update as a fraction (e.g., 0.01 = 1%)
    #[must_use]
    pub fn dampening_factor(mut self, factor: f64) -> Self {
        self.dampening_factor = Some(factor);
        self
    }

    /// Builds the configuration, validating all parameters.
    ///
    /// # Errors
    ///
    /// Returns `Error::ConfigurationError` if validation fails.
    pub fn build(self) -> Result<MarkPriceConfig> {
        let defaults = MarkPriceConfig::default();

        let config = MarkPriceConfig {
            index_weight: self.index_weight.unwrap_or(defaults.index_weight),
            mid_weight: self.mid_weight.unwrap_or(defaults.mid_weight),
            last_trade_weight: self.last_trade_weight.unwrap_or(defaults.last_trade_weight),
            dampening_factor: self.dampening_factor.unwrap_or(defaults.dampening_factor),
        };

        config.validate()?;
        Ok(config)
    }
}

/// Thread-safe mark price calculator.
///
/// Computes the mark price as a weighted average of index price, mid price,
/// and last trade price, with dampening to limit price movement.
///
/// ## Thread Safety
///
/// All price updates and reads use atomic operations, making this safe for
/// concurrent access from multiple threads without external synchronization.
/// The dampening logic uses a compare-and-swap loop to guarantee the
/// dampening invariant holds even under concurrent `mark_price()` calls.
///
/// Note that the three input prices (index, mid, last trade) are loaded
/// individually — they do not form an atomic snapshot. Under rapid concurrent
/// updates a mark price computation may see a mix of old and new inputs.
/// This is acceptable because mark price is recomputed frequently and the
/// inputs converge quickly.
///
/// ## Precision
///
/// Prices are stored as `u64` and converted to `f64` for the weighted
/// average calculation. Values above 2^53 (≈ 9 × 10^15) may lose
/// integer precision through the `f64` round-trip. For typical financial
/// prices in smallest units (satoshis, wei, cents) this is not a concern.
///
/// ## Example
///
/// ```
/// use option_chain_orderbook::orderbook::{MarkPriceCalculator, MarkPriceConfig};
///
/// let calculator = MarkPriceCalculator::with_default_config();
///
/// // Update prices
/// calculator.update_index_price(50000);
/// calculator.update_mid_price(50100);
/// calculator.update_last_trade_price(50050);
///
/// // Get mark price
/// if let Some(mark) = calculator.mark_price() {
///     println!("Mark price: {}", mark);
/// }
/// ```
pub struct MarkPriceCalculator {
    /// Configuration for weights and dampening.
    config: MarkPriceConfig,
    /// Latest index price (external reference).
    index_price: AtomicU64,
    /// Latest mid price (order book midpoint).
    mid_price: AtomicU64,
    /// Latest last trade price.
    last_trade_price: AtomicU64,
    /// Previously computed mark price for dampening.
    last_mark_price: AtomicU64,
}

impl MarkPriceCalculator {
    /// Creates a new calculator with the given configuration.
    ///
    /// # Arguments
    ///
    /// * `config` - Mark price configuration
    #[must_use]
    pub fn new(config: MarkPriceConfig) -> Self {
        Self {
            config,
            index_price: AtomicU64::new(0),
            mid_price: AtomicU64::new(0),
            last_trade_price: AtomicU64::new(0),
            last_mark_price: AtomicU64::new(0),
        }
    }

    /// Creates a new calculator with default configuration.
    ///
    /// Default weights: index=0.5, mid=0.3, last_trade=0.2
    /// Default dampening: 1% (0.01)
    #[must_use]
    pub fn with_default_config() -> Self {
        Self::new(MarkPriceConfig::default())
    }

    /// Returns a reference to the configuration.
    #[must_use]
    #[inline]
    pub fn config(&self) -> &MarkPriceConfig {
        &self.config
    }

    /// Updates the index price.
    ///
    /// # Arguments
    ///
    /// * `price` - New index price in smallest units
    #[inline]
    pub fn update_index_price(&self, price: u64) {
        self.index_price.store(price, Ordering::Release);
    }

    /// Updates the mid price (order book midpoint).
    ///
    /// # Arguments
    ///
    /// * `price` - New mid price in smallest units
    #[inline]
    pub fn update_mid_price(&self, price: u64) {
        self.mid_price.store(price, Ordering::Release);
    }

    /// Updates the last trade price.
    ///
    /// # Arguments
    ///
    /// * `price` - New last trade price in smallest units
    #[inline]
    pub fn update_last_trade_price(&self, price: u64) {
        self.last_trade_price.store(price, Ordering::Release);
    }

    /// Returns the current index price.
    #[must_use]
    #[inline]
    pub fn index_price(&self) -> u64 {
        self.index_price.load(Ordering::Acquire)
    }

    /// Returns the current mid price.
    #[must_use]
    #[inline]
    pub fn mid_price(&self) -> u64 {
        self.mid_price.load(Ordering::Acquire)
    }

    /// Returns the current last trade price.
    #[must_use]
    #[inline]
    pub fn last_trade_price(&self) -> u64 {
        self.last_trade_price.load(Ordering::Acquire)
    }

    /// Returns the last computed mark price (before dampening on current call).
    #[must_use]
    #[inline]
    pub fn last_mark_price(&self) -> u64 {
        self.last_mark_price.load(Ordering::Acquire)
    }

    /// Computes the mark price.
    ///
    /// Returns the weighted average of available prices, clamped by the
    /// dampening factor to limit how much the price can change per update.
    ///
    /// # Returns
    ///
    /// - `Some(price)` if at least one input price is non-zero
    /// - `None` if all input prices are zero
    ///
    /// # Algorithm
    ///
    /// 1. Load all input prices (individually atomic, not a consistent snapshot)
    /// 2. Compute weighted average, using only non-zero inputs
    /// 3. Re-normalize weights if some inputs are missing
    /// 4. Apply dampening via CAS loop: clamp change to ±ceil(prev × dampening_factor)
    /// 5. Store and return the new mark price
    #[must_use]
    pub fn mark_price(&self) -> Option<u64> {
        let index = self.index_price.load(Ordering::Acquire);
        let mid = self.mid_price.load(Ordering::Acquire);
        let last_trade = self.last_trade_price.load(Ordering::Acquire);

        // If all prices are zero, no mark price available
        if index == 0 && mid == 0 && last_trade == 0 {
            return None;
        }

        // Compute weighted sum, only including non-zero prices
        let mut weighted_sum: f64 = 0.0;
        let mut total_weight: f64 = 0.0;

        if index > 0 {
            weighted_sum += index as f64 * self.config.index_weight;
            total_weight += self.config.index_weight;
        }
        if mid > 0 {
            weighted_sum += mid as f64 * self.config.mid_weight;
            total_weight += self.config.mid_weight;
        }
        if last_trade > 0 {
            weighted_sum += last_trade as f64 * self.config.last_trade_weight;
            total_weight += self.config.last_trade_weight;
        }

        // Normalize if not all inputs are present
        let raw_mark = if total_weight > 0.0 {
            (weighted_sum / total_weight) as u64
        } else {
            return None;
        };

        // Apply dampening using a CAS loop so concurrent updates always
        // respect the dampening invariant relative to the latest stored value.
        let mut prev_mark = self.last_mark_price.load(Ordering::Acquire);
        loop {
            let final_mark = if prev_mark > 0 {
                let base_change = prev_mark as f64 * self.config.dampening_factor;
                let mut max_change = base_change.ceil() as u64;
                if max_change == 0 && raw_mark != prev_mark {
                    max_change = 1;
                }
                let min_price = prev_mark.saturating_sub(max_change);
                let max_price = prev_mark.saturating_add(max_change);
                raw_mark.clamp(min_price, max_price)
            } else {
                // First calculation, no dampening
                raw_mark
            };

            match self.last_mark_price.compare_exchange(
                prev_mark,
                final_mark,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Some(final_mark),
                Err(actual_prev) => {
                    // Another thread updated the mark price; retry with the
                    // latest value so dampening is applied correctly.
                    prev_mark = actual_prev;
                }
            }
        }
    }

    /// Resets all prices to zero.
    ///
    /// Useful for testing or when switching instruments.
    pub fn reset(&self) {
        self.index_price.store(0, Ordering::Release);
        self.mid_price.store(0, Ordering::Release);
        self.last_trade_price.store(0, Ordering::Release);
        self.last_mark_price.store(0, Ordering::Release);
    }
}

impl std::fmt::Debug for MarkPriceCalculator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MarkPriceCalculator")
            .field("config", &self.config)
            .field("index_price", &self.index_price.load(Ordering::Relaxed))
            .field("mid_price", &self.mid_price.load(Ordering::Relaxed))
            .field(
                "last_trade_price",
                &self.last_trade_price.load(Ordering::Relaxed),
            )
            .field(
                "last_mark_price",
                &self.last_mark_price.load(Ordering::Relaxed),
            )
            .finish()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    // ── MarkPriceConfig Tests ────────────────────────────────────────────

    #[test]
    fn test_default_config() {
        let config = MarkPriceConfig::default();
        assert!((config.index_weight() - 0.5).abs() < f64::EPSILON);
        assert!((config.mid_weight() - 0.3).abs() < f64::EPSILON);
        assert!((config.last_trade_weight() - 0.2).abs() < f64::EPSILON);
        assert!((config.dampening_factor() - 0.01).abs() < f64::EPSILON);
    }

    #[test]
    fn test_config_validation_valid() {
        let config = MarkPriceConfig {
            index_weight: 0.5,
            mid_weight: 0.3,
            last_trade_weight: 0.2,
            dampening_factor: 0.01,
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation_weights_dont_sum_to_one() {
        let config = MarkPriceConfig {
            index_weight: 0.5,
            mid_weight: 0.3,
            last_trade_weight: 0.3, // Sum = 1.1
            dampening_factor: 0.01,
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_weight_out_of_range() {
        let config = MarkPriceConfig {
            index_weight: 1.5, // > 1.0
            mid_weight: 0.0,
            last_trade_weight: -0.5, // < 0.0
            dampening_factor: 0.01,
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_dampening_zero() {
        let config = MarkPriceConfig {
            index_weight: 0.5,
            mid_weight: 0.3,
            last_trade_weight: 0.2,
            dampening_factor: 0.0, // Invalid
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_dampening_greater_than_one() {
        let config = MarkPriceConfig {
            index_weight: 0.5,
            mid_weight: 0.3,
            last_trade_weight: 0.2,
            dampening_factor: 1.5, // Invalid
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_nan_dampening() {
        let config = MarkPriceConfig {
            index_weight: 0.5,
            mid_weight: 0.3,
            last_trade_weight: 0.2,
            dampening_factor: f64::NAN,
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_nan_weight() {
        let config = MarkPriceConfig {
            index_weight: f64::NAN,
            mid_weight: 0.3,
            last_trade_weight: 0.2,
            dampening_factor: 0.01,
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_infinity_weight() {
        let config = MarkPriceConfig {
            index_weight: f64::INFINITY,
            mid_weight: 0.3,
            last_trade_weight: 0.2,
            dampening_factor: 0.01,
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_infinity_dampening() {
        let config = MarkPriceConfig {
            index_weight: 0.5,
            mid_weight: 0.3,
            last_trade_weight: 0.2,
            dampening_factor: f64::INFINITY,
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        let config = MarkPriceConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: MarkPriceConfig = serde_json::from_str(&json).unwrap();
        assert!((deserialized.index_weight() - config.index_weight()).abs() < f64::EPSILON);
        assert!((deserialized.mid_weight() - config.mid_weight()).abs() < f64::EPSILON);
    }

    // ── MarkPriceConfigBuilder Tests ─────────────────────────────────────

    #[test]
    fn test_builder_default_values() {
        let config = MarkPriceConfig::builder().build().unwrap();
        assert!((config.index_weight() - 0.5).abs() < f64::EPSILON);
        assert!((config.mid_weight() - 0.3).abs() < f64::EPSILON);
        assert!((config.last_trade_weight() - 0.2).abs() < f64::EPSILON);
    }

    #[test]
    fn test_builder_custom_values() {
        let config = MarkPriceConfig::builder()
            .index_weight(0.6)
            .mid_weight(0.25)
            .last_trade_weight(0.15)
            .dampening_factor(0.02)
            .build()
            .unwrap();

        assert!((config.index_weight() - 0.6).abs() < f64::EPSILON);
        assert!((config.mid_weight() - 0.25).abs() < f64::EPSILON);
        assert!((config.last_trade_weight() - 0.15).abs() < f64::EPSILON);
        assert!((config.dampening_factor() - 0.02).abs() < f64::EPSILON);
    }

    #[test]
    fn test_builder_invalid_weights() {
        let result = MarkPriceConfig::builder()
            .index_weight(0.5)
            .mid_weight(0.5)
            .last_trade_weight(0.5) // Sum = 1.5
            .build();
        assert!(result.is_err());
    }

    // ── MarkPriceCalculator Tests ────────────────────────────────────────

    #[test]
    fn test_calculator_creation() {
        let calc = MarkPriceCalculator::with_default_config();
        assert_eq!(calc.index_price(), 0);
        assert_eq!(calc.mid_price(), 0);
        assert_eq!(calc.last_trade_price(), 0);
    }

    #[test]
    fn test_calculator_no_prices() {
        let calc = MarkPriceCalculator::with_default_config();
        assert!(calc.mark_price().is_none());
    }

    #[test]
    fn test_calculator_all_prices_present() {
        let calc = MarkPriceCalculator::with_default_config();

        calc.update_index_price(50000);
        calc.update_mid_price(50000);
        calc.update_last_trade_price(50000);

        let mark = calc.mark_price();
        assert!(mark.is_some());
        // All same price, weighted average should equal the price
        assert_eq!(mark.unwrap(), 50000);
    }

    #[test]
    fn test_calculator_weighted_average() {
        // Weights: index=0.5, mid=0.3, last=0.2
        let calc = MarkPriceCalculator::with_default_config();

        calc.update_index_price(100);
        calc.update_mid_price(200);
        calc.update_last_trade_price(300);

        let mark = calc.mark_price().unwrap();
        // Expected: 100*0.5 + 200*0.3 + 300*0.2 = 50 + 60 + 60 = 170
        assert_eq!(mark, 170);
    }

    #[test]
    fn test_calculator_partial_prices_index_only() {
        let calc = MarkPriceCalculator::with_default_config();

        calc.update_index_price(50000);

        let mark = calc.mark_price();
        assert!(mark.is_some());
        // Only index present, should use full weight on index
        assert_eq!(mark.unwrap(), 50000);
    }

    #[test]
    fn test_calculator_partial_prices_mid_and_last() {
        let config = MarkPriceConfig::builder()
            .index_weight(0.4)
            .mid_weight(0.3)
            .last_trade_weight(0.3)
            .build()
            .unwrap();
        let calc = MarkPriceCalculator::new(config);

        calc.update_mid_price(100);
        calc.update_last_trade_price(200);

        let mark = calc.mark_price().unwrap();
        // Normalize weights: mid=0.3/(0.3+0.3)=0.5, last=0.5
        // Expected: 100*0.5 + 200*0.5 = 150
        assert_eq!(mark, 150);
    }

    #[test]
    fn test_calculator_dampening() {
        let config = MarkPriceConfig::builder()
            .index_weight(1.0)
            .mid_weight(0.0)
            .last_trade_weight(0.0)
            .dampening_factor(0.10) // 10% max change
            .build()
            .unwrap();
        let calc = MarkPriceCalculator::new(config);

        // First update: no dampening
        calc.update_index_price(1000);
        let mark1 = calc.mark_price().unwrap();
        assert_eq!(mark1, 1000);

        // Second update: try to jump to 2000 (100% increase)
        // Should be clamped to 1000 + 10% = 1100
        calc.update_index_price(2000);
        let mark2 = calc.mark_price().unwrap();
        assert_eq!(mark2, 1100);

        // Third update: continue toward 2000
        // From 1100, max is 1100 + 110 = 1210
        calc.update_index_price(2000);
        let mark3 = calc.mark_price().unwrap();
        assert_eq!(mark3, 1210);
    }

    #[test]
    fn test_calculator_dampening_decrease() {
        let config = MarkPriceConfig::builder()
            .index_weight(1.0)
            .mid_weight(0.0)
            .last_trade_weight(0.0)
            .dampening_factor(0.10) // 10% max change
            .build()
            .unwrap();
        let calc = MarkPriceCalculator::new(config);

        // First update
        calc.update_index_price(1000);
        let mark1 = calc.mark_price().unwrap();
        assert_eq!(mark1, 1000);

        // Try to drop to 500 (50% decrease)
        // Should be clamped to 1000 - 10% = 900
        calc.update_index_price(500);
        let mark2 = calc.mark_price().unwrap();
        assert_eq!(mark2, 900);
    }

    #[test]
    fn test_calculator_dampening_small_price() {
        let config = MarkPriceConfig::builder()
            .index_weight(1.0)
            .mid_weight(0.0)
            .last_trade_weight(0.0)
            .dampening_factor(0.001) // 0.1% max change
            .build()
            .unwrap();
        let calc = MarkPriceCalculator::new(config);

        // First update: set initial mark to 5 (small price)
        calc.update_index_price(5);
        let mark1 = calc.mark_price().unwrap();
        assert_eq!(mark1, 5);

        // Second update: try to jump to 10
        // Without ceil fix, max_change = (5 * 0.001) as u64 = 0, mark stuck at 5
        // With ceil fix, max_change = ceil(0.005) = 1, so mark can move to 6
        calc.update_index_price(10);
        let mark2 = calc.mark_price().unwrap();
        assert_eq!(mark2, 6);
    }

    #[test]
    fn test_calculator_reset() {
        let calc = MarkPriceCalculator::with_default_config();

        calc.update_index_price(50000);
        calc.update_mid_price(50100);
        calc.update_last_trade_price(50050);
        let _ = calc.mark_price();

        calc.reset();

        assert_eq!(calc.index_price(), 0);
        assert_eq!(calc.mid_price(), 0);
        assert_eq!(calc.last_trade_price(), 0);
        assert_eq!(calc.last_mark_price(), 0);
        assert!(calc.mark_price().is_none());
    }

    #[test]
    fn test_calculator_debug() {
        let calc = MarkPriceCalculator::with_default_config();
        calc.update_index_price(50000);
        let debug_str = format!("{:?}", calc);
        assert!(debug_str.contains("MarkPriceCalculator"));
        assert!(debug_str.contains("50000"));
    }

    #[test]
    fn test_calculator_thread_safety() {
        use std::sync::Arc;
        use std::thread;

        let calc = Arc::new(MarkPriceCalculator::with_default_config());
        let mut handles = vec![];

        // Spawn multiple threads updating prices
        for i in 0..10 {
            let calc_clone = Arc::clone(&calc);
            handles.push(thread::spawn(move || {
                for j in 0..100 {
                    let price = (i * 100 + j) as u64 * 100;
                    calc_clone.update_index_price(price);
                    calc_clone.update_mid_price(price);
                    calc_clone.update_last_trade_price(price);
                    let _ = calc_clone.mark_price();
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Should not panic or corrupt data
        let mark = calc.mark_price();
        assert!(mark.is_some());
    }

    #[test]
    fn test_equal_weights() {
        let config = MarkPriceConfig::builder()
            .index_weight(1.0 / 3.0)
            .mid_weight(1.0 / 3.0)
            .last_trade_weight(1.0 / 3.0)
            .build()
            .unwrap();
        let calc = MarkPriceCalculator::new(config);

        calc.update_index_price(100);
        calc.update_mid_price(200);
        calc.update_last_trade_price(300);

        let mark = calc.mark_price().unwrap();
        // Expected: (100 + 200 + 300) / 3 = 200
        assert_eq!(mark, 200);
    }

    #[test]
    fn test_zero_weight_ignored() {
        let config = MarkPriceConfig::builder()
            .index_weight(1.0)
            .mid_weight(0.0)
            .last_trade_weight(0.0)
            .build()
            .unwrap();
        let calc = MarkPriceCalculator::new(config);

        calc.update_index_price(1000);
        calc.update_mid_price(5000); // Should be ignored due to 0 weight
        calc.update_last_trade_price(9000); // Should be ignored

        let mark = calc.mark_price().unwrap();
        assert_eq!(mark, 1000);
    }
}
