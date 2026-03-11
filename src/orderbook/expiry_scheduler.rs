//! Expiry scheduling module.
//!
//! This module provides [`ExpiryScheduler`] for automatically creating missing
//! expiration order books based on [`ExpiryCycleConfig`] and generating strikes
//! using [`StrikeGenerator`].
//!
//! ## Algorithm
//!
//! 1. Generate expected expiration dates from config
//! 2. For each date, create expiration if missing
//! 3. If expiration is new (no strikes), generate strikes using spot price
//! 4. Invoke callback for each newly created expiration
//!
//! ## Example
//!
//! ```
//! use option_chain_orderbook::orderbook::{
//!     ExpiryScheduler, ExpiryCycleConfig, StrikeRangeConfig, UnderlyingOrderBook,
//! };
//! use chrono::Utc;
//!
//! let book = UnderlyingOrderBook::new("BTC");
//! let expiry_config = ExpiryCycleConfig::default();
//! let strike_config = StrikeRangeConfig::builder()
//!     .range_pct(0.10)
//!     .strike_interval(1000)
//!     .min_strikes(5)
//!     .max_strikes(50)
//!     .build()
//!     .expect("valid config");
//!
//! let result = ExpiryScheduler::refresh_expirations(
//!     &book,
//!     Utc::now(),
//!     &expiry_config,
//!     &strike_config,
//!     50000,
//!     None,
//! ).expect("refresh should succeed");
//!
//! assert!(!result.created.is_empty());
//! ```

use super::expiry_cycle::ExpiryCycleConfig;
use super::strike_generator::StrikeGenerator;
use super::strike_range::StrikeRangeConfig;
use super::underlying::UnderlyingOrderBook;
use crate::error::{Error, Result};
use chrono::{DateTime, Utc};
use optionstratlib::ExpirationDate;

// ─── RefreshResult ────────────────────────────────────────────────────────────

/// Result of a refresh operation.
///
/// Contains the list of newly created expiration dates and the total number
/// of strikes generated across all new expirations.
///
/// # Examples
///
/// ```
/// use option_chain_orderbook::orderbook::RefreshResult;
/// use optionstratlib::prelude::{ExpirationDate, Positive};
///
/// let result = RefreshResult {
///     created: vec![ExpirationDate::Days(Positive::THIRTY)],
///     strikes_generated: 11,
/// };
/// assert_eq!(result.created.len(), 1);
/// ```
#[derive(Debug, Clone, Default)]
pub struct RefreshResult {
    /// Expiration dates that were newly created.
    pub created: Vec<ExpirationDate>,
    /// Total number of strikes generated across all new expirations.
    pub strikes_generated: usize,
}

impl RefreshResult {
    /// Returns true if no expirations were created.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.created.is_empty()
    }

    /// Returns the number of expirations created.
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.created.len()
    }
}

// ─── ExpirationCallback ───────────────────────────────────────────────────────

/// Callback invoked for each newly created expiration.
///
/// The callback receives the expiration date that was just created.
pub type ExpirationCallback = Box<dyn Fn(&ExpirationDate) + Send + Sync>;

// ─── ExpiryScheduler ──────────────────────────────────────────────────────────

/// Zero-sized expiry scheduling utility.
///
/// Provides static methods for refreshing expirations on an underlying order
/// book. Creates missing expirations based on [`ExpiryCycleConfig`] and
/// generates strikes for new expirations using [`StrikeGenerator`].
///
/// All operations are idempotent: calling them multiple times produces the
/// same result as calling once.
pub struct ExpiryScheduler;

impl ExpiryScheduler {
    /// Refreshes expirations for an underlying, creating missing ones and
    /// generating strikes for new expirations.
    ///
    /// # Algorithm
    ///
    /// 1. Generate expected dates from `expiry_config.generate_dates(now)`
    /// 2. For each date, check if expiration already exists
    /// 3. If new, create expiration and generate strikes
    /// 4. Invoke callback for each newly created expiration
    ///
    /// # Arguments
    ///
    /// * `underlying` - The underlying order book to refresh
    /// * `now` - Current datetime for computing expected expirations
    /// * `expiry_config` - Configuration for which expirations to create
    /// * `strike_config` - Configuration for strike generation
    /// * `spot_price` - Current spot price for strike generation
    /// * `callback` - Optional callback invoked for each new expiration
    ///
    /// # Returns
    ///
    /// A [`RefreshResult`] containing the created expirations and strike count.
    ///
    /// # Errors
    ///
    /// Returns `Error::ConfigurationError` if:
    /// - `expiry_config` validation fails
    /// - `strike_config` validation fails
    /// - Strike generation fails
    ///
    /// # Examples
    ///
    /// ```
    /// use option_chain_orderbook::orderbook::{
    ///     ExpiryScheduler, ExpiryCycleConfig, StrikeRangeConfig, UnderlyingOrderBook,
    /// };
    /// use chrono::Utc;
    ///
    /// let book = UnderlyingOrderBook::new("BTC");
    /// let expiry_config = ExpiryCycleConfig::default();
    /// let strike_config = StrikeRangeConfig::builder()
    ///     .range_pct(0.10)
    ///     .strike_interval(1000)
    ///     .min_strikes(5)
    ///     .max_strikes(50)
    ///     .build()
    ///     .expect("valid config");
    ///
    /// let result = ExpiryScheduler::refresh_expirations(
    ///     &book,
    ///     Utc::now(),
    ///     &expiry_config,
    ///     &strike_config,
    ///     50000,
    ///     None,
    /// ).expect("refresh should succeed");
    ///
    /// // Default config creates multiple expirations (daily, weekly, monthly, quarterly)
    /// assert!(!result.created.is_empty());
    /// ```
    pub fn refresh_expirations(
        underlying: &UnderlyingOrderBook,
        now: DateTime<Utc>,
        expiry_config: &ExpiryCycleConfig,
        strike_config: &StrikeRangeConfig,
        spot_price: u64,
        callback: Option<&ExpirationCallback>,
    ) -> Result<RefreshResult> {
        // Generate expected dates from config
        let expected_dates = expiry_config.generate_dates(now)?;

        let mut result = RefreshResult::default();

        for date in expected_dates {
            // Check if expiration already exists (not just empty)
            let is_new = underlying.get_expiration(&date).is_err();

            // Get or create the expiration
            let exp = underlying.get_or_create_expiration(date);

            // Only process truly new expirations (not pre-existing empty ones)
            if is_new {
                // Generate and apply strikes
                let strikes =
                    StrikeGenerator::refresh_strikes(exp.chain(), spot_price, strike_config)?;

                result.strikes_generated = result
                    .strikes_generated
                    .checked_add(strikes.len())
                    .ok_or_else(|| Error::configuration("strikes_generated overflow"))?;

                result.created.push(date);

                // Invoke callback if provided
                if let Some(cb) = callback {
                    cb(&date);
                }
            }
        }

        Ok(result)
    }

    /// Refreshes expirations using configs stored on the underlying.
    ///
    /// This is a convenience method that retrieves the expiry cycle and strike
    /// range configurations from the underlying order book.
    ///
    /// # Arguments
    ///
    /// * `underlying` - The underlying order book (must have configs set)
    /// * `now` - Current datetime for computing expected expirations
    /// * `spot_price` - Current spot price for strike generation
    /// * `callback` - Optional callback invoked for each new expiration
    ///
    /// # Errors
    ///
    /// Returns `Error::ConfigurationError` if:
    /// - Expiry cycle config is not set on the underlying
    /// - No strike range configs are set on the underlying
    /// - Any config validation fails
    ///
    /// # Examples
    ///
    /// ```
    /// use option_chain_orderbook::orderbook::{
    ///     ExpiryScheduler, ExpiryCycleConfig, ExpiryType, StrikeRangeConfig,
    ///     UnderlyingOrderBook,
    /// };
    /// use chrono::Utc;
    ///
    /// let book = UnderlyingOrderBook::new("BTC");
    ///
    /// // Set configs on the underlying
    /// book.set_expiry_cycle_config(ExpiryCycleConfig::default()).expect("valid");
    /// book.set_strike_range_config(
    ///     ExpiryType::Daily,
    ///     StrikeRangeConfig::builder()
    ///         .range_pct(0.10)
    ///         .strike_interval(1000)
    ///         .build()
    ///         .expect("valid"),
    /// ).expect("valid");
    ///
    /// let result = ExpiryScheduler::refresh_from_underlying(&book, Utc::now(), 50000, None)
    ///     .expect("refresh should succeed");
    /// ```
    pub fn refresh_from_underlying(
        underlying: &UnderlyingOrderBook,
        now: DateTime<Utc>,
        spot_price: u64,
        callback: Option<&ExpirationCallback>,
    ) -> Result<RefreshResult> {
        // Get expiry cycle config
        let expiry_config = underlying
            .expiry_cycle_config()
            .ok_or_else(|| Error::configuration("expiry cycle config not set on underlying"))?;

        // Get strike range configs - require exactly one to avoid nondeterministic selection
        let strike_configs = underlying.strike_range_configs();
        let strike_config = match strike_configs.len() {
            0 => {
                return Err(Error::configuration(
                    "no strike range configs set on underlying",
                ));
            }
            1 => strike_configs
                .values()
                .next()
                .cloned()
                .expect("len == 1 but no value"),
            _ => {
                return Err(Error::configuration(
                    "multiple strike range configs set; refresh_from_underlying requires exactly one",
                ));
            }
        };

        Self::refresh_expirations(
            underlying,
            now,
            &expiry_config,
            &strike_config,
            spot_price,
            callback,
        )
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orderbook::{CycleRule, ExpiryType};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn default_strike_config() -> StrikeRangeConfig {
        StrikeRangeConfig::builder()
            .range_pct(0.10)
            .strike_interval(1000)
            .min_strikes(5)
            .max_strikes(50)
            .build()
            .expect("valid config")
    }

    fn minimal_expiry_config() -> ExpiryCycleConfig {
        ExpiryCycleConfig {
            cycles: vec![CycleRule {
                cycle_type: ExpiryType::Daily,
                count: 2,
            }],
            expiry_time_utc: (8, 0),
            settlement_time_utc: (8, 30),
        }
    }

    // ── refresh_expirations basic ─────────────────────────────────────────────

    #[test]
    fn test_refresh_creates_expirations() {
        let book = UnderlyingOrderBook::new("BTC");
        let expiry_config = minimal_expiry_config();
        let strike_config = default_strike_config();

        let result = ExpiryScheduler::refresh_expirations(
            &book,
            Utc::now(),
            &expiry_config,
            &strike_config,
            50000,
            None,
        )
        .expect("refresh should succeed");

        // Should create 2 daily expirations
        assert_eq!(result.created.len(), 2);
        assert!(result.strikes_generated > 0);
        assert_eq!(book.expiration_count(), 2);
    }

    #[test]
    fn test_refresh_generates_strikes() {
        let book = UnderlyingOrderBook::new("BTC");
        let expiry_config = minimal_expiry_config();
        let strike_config = default_strike_config();

        let result = ExpiryScheduler::refresh_expirations(
            &book,
            Utc::now(),
            &expiry_config,
            &strike_config,
            50000,
            None,
        )
        .expect("refresh should succeed");

        // Each expiration should have strikes
        for date in &result.created {
            let exp = book.get_expiration(date).expect("expiration exists");
            assert!(!exp.is_empty(), "expiration should have strikes");
        }
    }

    #[test]
    fn test_refresh_is_idempotent() {
        let book = UnderlyingOrderBook::new("BTC");
        let expiry_config = minimal_expiry_config();
        let strike_config = default_strike_config();
        let now = Utc::now(); // Capture once to avoid flakiness across day boundary

        // First refresh
        let result1 = ExpiryScheduler::refresh_expirations(
            &book,
            now,
            &expiry_config,
            &strike_config,
            50000,
            None,
        )
        .expect("first refresh should succeed");

        assert_eq!(result1.created.len(), 2);

        // Second refresh - same config, same time
        let result2 = ExpiryScheduler::refresh_expirations(
            &book,
            now,
            &expiry_config,
            &strike_config,
            50000,
            None,
        )
        .expect("second refresh should succeed");

        // No new expirations should be created
        assert!(
            result2.created.is_empty(),
            "idempotent refresh should not create new expirations"
        );
        assert_eq!(result2.strikes_generated, 0);

        // Total expiration count unchanged
        assert_eq!(book.expiration_count(), 2);
    }

    #[test]
    fn test_existing_expirations_untouched() {
        let book = UnderlyingOrderBook::new("BTC");
        let expiry_config = minimal_expiry_config();
        let strike_config = default_strike_config();
        let now = Utc::now(); // Capture once to avoid flakiness across day boundary

        // First refresh
        let result1 = ExpiryScheduler::refresh_expirations(
            &book,
            now,
            &expiry_config,
            &strike_config,
            50000,
            None,
        )
        .expect("first refresh should succeed");

        // Get strike counts for each expiration
        let original_strikes: Vec<_> = result1
            .created
            .iter()
            .map(|d| {
                book.get_expiration(d)
                    .expect("exists")
                    .chain()
                    .strike_count()
            })
            .collect();

        // Second refresh with different spot price
        let _ = ExpiryScheduler::refresh_expirations(
            &book,
            now,
            &expiry_config,
            &strike_config,
            60000, // Different spot
            None,
        )
        .expect("second refresh should succeed");

        // Strike counts should be unchanged
        for (i, date) in result1.created.iter().enumerate() {
            let current_strikes = book
                .get_expiration(date)
                .expect("exists")
                .chain()
                .strike_count();
            assert_eq!(
                current_strikes, original_strikes[i],
                "existing expiration strikes should not change"
            );
        }
    }

    #[test]
    fn test_empty_config_returns_error() {
        let book = UnderlyingOrderBook::new("BTC");
        let expiry_config = ExpiryCycleConfig {
            cycles: vec![],
            expiry_time_utc: (8, 0),
            settlement_time_utc: (8, 30),
        };
        let strike_config = default_strike_config();

        let result = ExpiryScheduler::refresh_expirations(
            &book,
            Utc::now(),
            &expiry_config,
            &strike_config,
            50000,
            None,
        );

        // Empty cycles config is invalid per ExpiryCycleConfig::validate()
        assert!(result.is_err(), "empty cycles should fail validation");
    }

    #[test]
    fn test_callback_invoked_for_new_expirations() {
        let book = UnderlyingOrderBook::new("BTC");
        let expiry_config = minimal_expiry_config();
        let strike_config = default_strike_config();

        let callback_count = Arc::new(AtomicUsize::new(0));
        let callback_count_clone = Arc::clone(&callback_count);

        let callback: ExpirationCallback = Box::new(move |_date| {
            callback_count_clone.fetch_add(1, Ordering::SeqCst);
        });

        let result = ExpiryScheduler::refresh_expirations(
            &book,
            Utc::now(),
            &expiry_config,
            &strike_config,
            50000,
            Some(&callback),
        )
        .expect("refresh should succeed");

        assert_eq!(
            callback_count.load(Ordering::SeqCst),
            result.created.len(),
            "callback should be invoked for each new expiration"
        );
    }

    #[test]
    fn test_callback_not_invoked_for_existing() {
        let book = UnderlyingOrderBook::new("BTC");
        let expiry_config = minimal_expiry_config();
        let strike_config = default_strike_config();
        let now = Utc::now(); // Capture once to avoid flakiness across day boundary

        // First refresh without callback
        let _ = ExpiryScheduler::refresh_expirations(
            &book,
            now,
            &expiry_config,
            &strike_config,
            50000,
            None,
        )
        .expect("first refresh should succeed");

        // Second refresh with callback
        let callback_count = Arc::new(AtomicUsize::new(0));
        let callback_count_clone = Arc::clone(&callback_count);

        let callback: ExpirationCallback = Box::new(move |_date| {
            callback_count_clone.fetch_add(1, Ordering::SeqCst);
        });

        let _ = ExpiryScheduler::refresh_expirations(
            &book,
            now,
            &expiry_config,
            &strike_config,
            50000,
            Some(&callback),
        )
        .expect("second refresh should succeed");

        assert_eq!(
            callback_count.load(Ordering::SeqCst),
            0,
            "callback should not be invoked for existing expirations"
        );
    }

    // ── refresh_from_underlying ───────────────────────────────────────────────

    #[test]
    fn test_refresh_from_underlying_works() {
        let book = UnderlyingOrderBook::new("BTC");

        // Set configs on underlying
        book.set_expiry_cycle_config(minimal_expiry_config())
            .expect("valid config");
        book.set_strike_range_config(ExpiryType::Daily, default_strike_config())
            .expect("valid config");

        let result = ExpiryScheduler::refresh_from_underlying(&book, Utc::now(), 50000, None)
            .expect("refresh should succeed");

        assert_eq!(result.created.len(), 2);
    }

    #[test]
    fn test_refresh_from_underlying_missing_expiry_config() {
        let book = UnderlyingOrderBook::new("BTC");

        // Set only strike config
        book.set_strike_range_config(ExpiryType::Daily, default_strike_config())
            .expect("valid config");

        let result = ExpiryScheduler::refresh_from_underlying(&book, Utc::now(), 50000, None);

        assert!(result.is_err(), "should fail without expiry cycle config");
    }

    #[test]
    fn test_refresh_from_underlying_missing_strike_config() {
        let book = UnderlyingOrderBook::new("BTC");

        // Set only expiry config
        book.set_expiry_cycle_config(minimal_expiry_config())
            .expect("valid config");

        let result = ExpiryScheduler::refresh_from_underlying(&book, Utc::now(), 50000, None);

        assert!(result.is_err(), "should fail without strike range config");
    }

    // ── RefreshResult ─────────────────────────────────────────────────────────

    #[test]
    fn test_refresh_result_default() {
        let result = RefreshResult::default();
        assert!(result.is_empty());
        assert_eq!(result.len(), 0);
        assert_eq!(result.strikes_generated, 0);
    }
}
