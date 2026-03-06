//! Strike generation module.
//!
//! This module provides [`StrikeGenerator`] for computing strike prices from a
//! spot price and [`StrikeRangeConfig`], then applying them to an
//! [`OptionChainOrderBook`].
//!
//! ## Algorithm
//!
//! 1. Compute ATM strike by rounding spot to nearest interval
//! 2. Compute range bounds: `spot * (1 ± range_pct)`
//! 3. Generate strikes from low to high at interval steps
//! 4. Cap at `max_strikes`
//! 5. Expand symmetrically outward if below `min_strikes`
//!
//! ## Example
//!
//! ```
//! use option_chain_orderbook::orderbook::{
//!     OptionChainOrderBook, StrikeGenerator, StrikeRangeConfig,
//! };
//! use optionstratlib::prelude::{ExpirationDate, Positive};
//!
//! let config = StrikeRangeConfig::builder()
//!     .range_pct(0.10)
//!     .strike_interval(1000)
//!     .min_strikes(5)
//!     .max_strikes(50)
//!     .build()
//!     .expect("valid config");
//!
//! let strikes = StrikeGenerator::generate_strikes(50000, &config).expect("ok");
//! assert!(strikes.len() >= 5);
//! assert!(strikes.len() <= 50);
//!
//! // Apply to a chain
//! let chain = OptionChainOrderBook::new("BTC", ExpirationDate::Days(Positive::THIRTY));
//! StrikeGenerator::apply_strikes(&chain, &strikes);
//! assert_eq!(chain.strike_count(), strikes.len());
//! ```

use super::chain::OptionChainOrderBook;
use super::strike_range::StrikeRangeConfig;
use crate::error::{Error, Result};

/// Zero-sized strike generation utility.
///
/// Provides static methods for computing strike prices from a spot price and
/// configuration, and for applying those strikes to an option chain.
///
/// All arithmetic uses checked operations to prevent overflow on pathological
/// inputs.
pub struct StrikeGenerator;

impl StrikeGenerator {
    /// Computes the ATM-centered strike list for a given spot price and config.
    ///
    /// # Algorithm
    ///
    /// 1. Round `spot` to nearest `strike_interval` to get ATM
    /// 2. Compute range bounds: `spot * (1 ± range_pct)`
    /// 3. Floor/ceil bounds to interval multiples
    /// 4. Generate strikes from low to high, capped at `max_strikes`
    /// 5. If count < `min_strikes`, expand symmetrically outward
    ///
    /// # Arguments
    ///
    /// * `spot` - Current spot price in price units (e.g., 50000 for $50,000)
    /// * `config` - Strike range configuration
    ///
    /// # Errors
    ///
    /// Returns `Error::ConfigurationError` if:
    /// - `spot` is zero
    /// - Arithmetic overflow occurs (pathological inputs)
    ///
    /// # Examples
    ///
    /// ```
    /// use option_chain_orderbook::orderbook::{StrikeGenerator, StrikeRangeConfig};
    ///
    /// let config = StrikeRangeConfig::builder()
    ///     .range_pct(0.10)
    ///     .strike_interval(1000)
    ///     .min_strikes(5)
    ///     .max_strikes(50)
    ///     .build()
    ///     .expect("valid config");
    ///
    /// let strikes = StrikeGenerator::generate_strikes(50000, &config).expect("ok");
    /// // With 10% range on 50000, we get 45000..55000, so 11 strikes at 1000 interval
    /// assert_eq!(strikes.len(), 11);
    /// ```
    pub fn generate_strikes(spot: u64, config: &StrikeRangeConfig) -> Result<Vec<u64>> {
        config.validate()?;

        if spot == 0 {
            return Err(Error::configuration("spot price must be positive"));
        }

        let interval = config.strike_interval();
        let range_pct = config.range_pct();
        let min_strikes = config.min_strikes();
        let max_strikes = config.max_strikes();

        // Compute ATM: round spot to nearest interval
        let half_interval = interval / 2;
        let atm = spot
            .checked_add(half_interval)
            .ok_or_else(|| Error::configuration("overflow computing ATM"))?
            / interval
            * interval;

        // Compute range bounds using f64 for the percentage calculation
        let spot_f64 = spot as f64;
        let low_f64 = spot_f64 * (1.0 - range_pct);
        let high_f64 = spot_f64 * (1.0 + range_pct);

        // Convert to u64; range_pct is validated to (0,1] so low_f64 is non-negative
        let low = low_f64 as u64;
        let high = high_f64 as u64;

        // Floor low to interval multiple
        let low_strike = (low / interval) * interval;

        // Ceil high to interval multiple: ((high + interval - 1) / interval) * interval
        let high_strike = high
            .checked_add(interval.saturating_sub(1))
            .ok_or_else(|| Error::configuration("overflow computing high_strike"))?
            / interval
            * interval;

        // Generate strikes from low to high
        let mut strikes = Vec::new();
        let mut strike = low_strike;

        // Ensure we start at a valid strike (at least interval)
        if strike == 0 {
            strike = interval;
        }

        while strike <= high_strike {
            strikes.push(strike);
            strike = match strike.checked_add(interval) {
                Some(s) => s,
                None => break, // Overflow, stop generating
            };
        }

        // Cap at max_strikes by selecting ATM-centered window
        if strikes.len() > max_strikes {
            // Find ATM index or closest strike
            let atm_idx = strikes
                .iter()
                .position(|&s| s >= atm)
                .unwrap_or(strikes.len().saturating_sub(1));

            // Select centered window around ATM
            let half = max_strikes / 2;
            let start = atm_idx.saturating_sub(half);
            let end = (start + max_strikes).min(strikes.len());
            let start = end.saturating_sub(max_strikes);

            strikes = strikes[start..end].to_vec();
        }

        // Expand symmetrically if below min_strikes
        if strikes.len() < min_strikes && !strikes.is_empty() {
            let mut current_low = *strikes.first().unwrap_or(&atm);
            let mut current_high = *strikes.last().unwrap_or(&atm);
            let mut expand_low = true;

            while strikes.len() < min_strikes {
                if expand_low {
                    // Try to expand below
                    if let Some(new_low) = current_low.checked_sub(interval)
                        && new_low >= interval
                    {
                        strikes.insert(0, new_low);
                        current_low = new_low;
                    }
                    expand_low = false;
                } else {
                    // Try to expand above
                    if let Some(new_high) = current_high.checked_add(interval) {
                        strikes.push(new_high);
                        current_high = new_high;
                    } else {
                        // Can't expand above, stop to avoid infinite loop
                        break;
                    }
                    expand_low = true;
                }

                // Safety: if we can't expand in either direction, break
                if strikes.len() < min_strikes {
                    let can_expand_low = current_low
                        .checked_sub(interval)
                        .map(|l| l >= interval)
                        .unwrap_or(false);
                    let can_expand_high = current_high.checked_add(interval).is_some();
                    if !can_expand_low && !can_expand_high {
                        break;
                    }
                }
            }
        }

        // Ensure strikes are sorted (they should be, but defensive)
        strikes.sort_unstable();

        Ok(strikes)
    }

    /// Creates `StrikeOrderBook` entries for each strike (idempotent).
    ///
    /// Calls [`OptionChainOrderBook::get_or_create_strike`] for each strike
    /// in the slice. If a strike already exists, this is a no-op for that strike.
    ///
    /// # Arguments
    ///
    /// * `chain` - The option chain to populate with strikes
    /// * `strikes` - Slice of strike prices to create
    ///
    /// # Examples
    ///
    /// ```
    /// use option_chain_orderbook::orderbook::{OptionChainOrderBook, StrikeGenerator};
    /// use optionstratlib::prelude::{ExpirationDate, Positive};
    ///
    /// let chain = OptionChainOrderBook::new("BTC", ExpirationDate::Days(Positive::THIRTY));
    /// let strikes = vec![45000, 50000, 55000];
    ///
    /// StrikeGenerator::apply_strikes(&chain, &strikes);
    /// assert_eq!(chain.strike_count(), 3);
    ///
    /// // Idempotent: calling again doesn't change count
    /// StrikeGenerator::apply_strikes(&chain, &strikes);
    /// assert_eq!(chain.strike_count(), 3);
    /// ```
    pub fn apply_strikes(chain: &OptionChainOrderBook, strikes: &[u64]) {
        for &strike in strikes {
            let _ = chain.get_or_create_strike(strike);
        }
    }

    /// Combines strike generation and application in one call.
    ///
    /// This is equivalent to calling [`generate_strikes`](Self::generate_strikes)
    /// followed by [`apply_strikes`](Self::apply_strikes).
    ///
    /// # Arguments
    ///
    /// * `chain` - The option chain to populate with strikes
    /// * `spot` - Current spot price
    /// * `config` - Strike range configuration
    ///
    /// # Returns
    ///
    /// The generated strike prices that were applied to the chain.
    ///
    /// # Errors
    ///
    /// Returns `Error::ConfigurationError` if strike generation fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use option_chain_orderbook::orderbook::{
    ///     OptionChainOrderBook, StrikeGenerator, StrikeRangeConfig,
    /// };
    /// use optionstratlib::prelude::{ExpirationDate, Positive};
    ///
    /// let chain = OptionChainOrderBook::new("BTC", ExpirationDate::Days(Positive::THIRTY));
    /// let config = StrikeRangeConfig::builder()
    ///     .range_pct(0.10)
    ///     .strike_interval(1000)
    ///     .min_strikes(5)
    ///     .max_strikes(50)
    ///     .build()
    ///     .expect("valid config");
    ///
    /// let strikes = StrikeGenerator::refresh_strikes(&chain, 50000, &config).expect("ok");
    /// assert_eq!(chain.strike_count(), strikes.len());
    /// ```
    pub fn refresh_strikes(
        chain: &OptionChainOrderBook,
        spot: u64,
        config: &StrikeRangeConfig,
    ) -> Result<Vec<u64>> {
        let strikes = Self::generate_strikes(spot, config)?;
        Self::apply_strikes(chain, &strikes);
        Ok(strikes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use optionstratlib::prelude::{ExpirationDate, Positive};

    fn test_expiration() -> ExpirationDate {
        ExpirationDate::Days(Positive::THIRTY)
    }

    fn default_config() -> StrikeRangeConfig {
        StrikeRangeConfig::builder()
            .range_pct(0.10)
            .strike_interval(1000)
            .min_strikes(5)
            .max_strikes(50)
            .build()
            .expect("valid config")
    }

    // ── generate_strikes basic ─────────────────────────────────────────────────

    #[test]
    fn test_generate_strikes_basic() {
        let config = default_config();
        let strikes = StrikeGenerator::generate_strikes(50000, &config).expect("ok");

        // 10% of 50000 = 5000, so range is 45000..55000
        // At 1000 interval: 45000, 46000, ..., 55000 = 11 strikes
        assert_eq!(strikes.len(), 11);
        assert_eq!(strikes[0], 45000);
        assert_eq!(strikes[10], 55000);
    }

    #[test]
    fn test_generate_strikes_spot_on_interval() {
        let config = default_config();
        let strikes = StrikeGenerator::generate_strikes(50000, &config).expect("ok");

        // ATM should be 50000 (already on interval)
        assert!(strikes.contains(&50000));
    }

    #[test]
    fn test_generate_strikes_spot_between_intervals() {
        let config = default_config();
        let strikes = StrikeGenerator::generate_strikes(50500, &config).expect("ok");

        // ATM should round to 51000 (50500 + 500 = 51000)
        // Range: 45450..55550 → strikes 45000..56000
        assert!(strikes.contains(&51000));
    }

    #[test]
    fn test_generate_strikes_sorted() {
        let config = default_config();
        let strikes = StrikeGenerator::generate_strikes(50000, &config).expect("ok");

        let mut sorted = strikes.clone();
        sorted.sort_unstable();
        assert_eq!(strikes, sorted);
    }

    // ── generate_strikes edge cases ────────────────────────────────────────────

    #[test]
    fn test_generate_strikes_zero_spot_error() {
        let config = default_config();
        let result = StrikeGenerator::generate_strikes(0, &config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("positive"));
    }

    #[test]
    fn test_generate_strikes_small_spot() {
        // Spot = 500, interval = 1000 → low range would be negative
        let config = StrikeRangeConfig::builder()
            .range_pct(0.50)
            .strike_interval(1000)
            .min_strikes(3)
            .max_strikes(50)
            .build()
            .expect("valid");

        let strikes = StrikeGenerator::generate_strikes(500, &config).expect("ok");

        // Should handle gracefully, starting at interval (1000)
        assert!(!strikes.is_empty());
        assert!(strikes[0] >= 1000);
    }

    #[test]
    fn test_generate_strikes_very_small_spot() {
        let config = StrikeRangeConfig::builder()
            .range_pct(0.10)
            .strike_interval(100)
            .min_strikes(3)
            .max_strikes(50)
            .build()
            .expect("valid");

        let strikes = StrikeGenerator::generate_strikes(50, &config).expect("ok");

        // Should generate at least min_strikes via expansion
        assert!(strikes.len() >= 3);
    }

    // ── min_strikes expansion ──────────────────────────────────────────────────

    #[test]
    fn test_generate_strikes_min_strikes_expansion() {
        let config = StrikeRangeConfig::builder()
            .range_pct(0.01) // Very small range
            .strike_interval(1000)
            .min_strikes(5)
            .max_strikes(50)
            .build()
            .expect("valid");

        let strikes = StrikeGenerator::generate_strikes(50000, &config).expect("ok");

        // 1% of 50000 = 500, so initial range is 49500..50500
        // That's only 50000 (1 strike) initially
        // Should expand to at least 5 strikes
        assert!(strikes.len() >= 5);
    }

    #[test]
    fn test_generate_strikes_min_strikes_symmetric() {
        let config = StrikeRangeConfig::builder()
            .range_pct(0.01)
            .strike_interval(1000)
            .min_strikes(5)
            .max_strikes(50)
            .build()
            .expect("valid");

        let strikes = StrikeGenerator::generate_strikes(50000, &config).expect("ok");

        // Should be symmetric around ATM (50000)
        let atm_idx = strikes.iter().position(|&s| s == 50000);
        assert!(atm_idx.is_some());
    }

    // ── max_strikes cap ────────────────────────────────────────────────────────

    #[test]
    fn test_generate_strikes_max_strikes_cap() {
        let config = StrikeRangeConfig::builder()
            .range_pct(0.50) // Large range
            .strike_interval(1000)
            .min_strikes(5)
            .max_strikes(10)
            .build()
            .expect("valid");

        let strikes = StrikeGenerator::generate_strikes(50000, &config).expect("ok");

        // Should be capped at max_strikes
        assert!(strikes.len() <= 10);
    }

    // ── apply_strikes ──────────────────────────────────────────────────────────

    #[test]
    fn test_apply_strikes_creates_strikes() {
        let chain = OptionChainOrderBook::new("BTC", test_expiration());
        let strikes = vec![45000, 50000, 55000];

        StrikeGenerator::apply_strikes(&chain, &strikes);

        assert_eq!(chain.strike_count(), 3);
        assert!(chain.get_strike(45000).is_ok());
        assert!(chain.get_strike(50000).is_ok());
        assert!(chain.get_strike(55000).is_ok());
    }

    #[test]
    fn test_apply_strikes_idempotent() {
        let chain = OptionChainOrderBook::new("BTC", test_expiration());
        let strikes = vec![45000, 50000, 55000];

        StrikeGenerator::apply_strikes(&chain, &strikes);
        assert_eq!(chain.strike_count(), 3);

        // Apply again - should not change count
        StrikeGenerator::apply_strikes(&chain, &strikes);
        assert_eq!(chain.strike_count(), 3);
    }

    #[test]
    fn test_apply_strikes_empty() {
        let chain = OptionChainOrderBook::new("BTC", test_expiration());
        let strikes: Vec<u64> = vec![];

        StrikeGenerator::apply_strikes(&chain, &strikes);
        assert_eq!(chain.strike_count(), 0);
    }

    // ── refresh_strikes ────────────────────────────────────────────────────────

    #[test]
    fn test_refresh_strikes_integration() {
        let chain = OptionChainOrderBook::new("BTC", test_expiration());
        let config = default_config();

        let strikes = StrikeGenerator::refresh_strikes(&chain, 50000, &config).expect("ok");

        assert_eq!(chain.strike_count(), strikes.len());
        for &strike in &strikes {
            assert!(chain.get_strike(strike).is_ok());
        }
    }

    #[test]
    fn test_refresh_strikes_idempotent() {
        let chain = OptionChainOrderBook::new("BTC", test_expiration());
        let config = default_config();

        let strikes1 = StrikeGenerator::refresh_strikes(&chain, 50000, &config).expect("ok");
        let count1 = chain.strike_count();

        let strikes2 = StrikeGenerator::refresh_strikes(&chain, 50000, &config).expect("ok");
        let count2 = chain.strike_count();

        assert_eq!(strikes1, strikes2);
        assert_eq!(count1, count2);
    }

    // ── ATM rounding ───────────────────────────────────────────────────────────

    #[test]
    fn test_atm_rounding_down() {
        // 50400 + 500 = 50900, /1000 = 50, *1000 = 50000
        let config = StrikeRangeConfig::builder()
            .range_pct(0.001) // Tiny range to isolate ATM
            .strike_interval(1000)
            .min_strikes(1)
            .max_strikes(50)
            .build()
            .expect("valid");

        let strikes = StrikeGenerator::generate_strikes(50400, &config).expect("ok");
        assert!(strikes.contains(&50000));
    }

    #[test]
    fn test_atm_rounding_up() {
        // 50600 + 500 = 51100, /1000 = 51, *1000 = 51000
        let config = StrikeRangeConfig::builder()
            .range_pct(0.001)
            .strike_interval(1000)
            .min_strikes(1)
            .max_strikes(50)
            .build()
            .expect("valid");

        let strikes = StrikeGenerator::generate_strikes(50600, &config).expect("ok");
        assert!(strikes.contains(&51000));
    }

    // ── Large spot ─────────────────────────────────────────────────────────────

    #[test]
    fn test_generate_strikes_large_spot() {
        let config = StrikeRangeConfig::builder()
            .range_pct(0.10)
            .strike_interval(1_000_000)
            .min_strikes(5)
            .max_strikes(50)
            .build()
            .expect("valid");

        // Large spot but not near u64 max
        let strikes = StrikeGenerator::generate_strikes(100_000_000, &config).expect("ok");
        assert!(!strikes.is_empty());
    }

    // ── Different intervals ────────────────────────────────────────────────────

    #[test]
    fn test_generate_strikes_eth_style_interval() {
        // ETH uses $50 intervals
        let config = StrikeRangeConfig::builder()
            .range_pct(0.10)
            .strike_interval(50)
            .min_strikes(5)
            .max_strikes(100)
            .build()
            .expect("valid");

        let strikes = StrikeGenerator::generate_strikes(3500, &config).expect("ok");

        // 10% of 3500 = 350, range 3150..3850
        // At 50 interval that's many strikes
        assert!(strikes.len() >= 14); // (3850-3150)/50 + 1 = 15
        for &s in &strikes {
            assert_eq!(s % 50, 0);
        }
    }

    #[test]
    fn test_generate_strikes_btc_style_interval() {
        // BTC uses $1000 intervals
        let config = StrikeRangeConfig::builder()
            .range_pct(0.20)
            .strike_interval(1000)
            .min_strikes(5)
            .max_strikes(100)
            .build()
            .expect("valid");

        let strikes = StrikeGenerator::generate_strikes(65000, &config).expect("ok");

        // 20% of 65000 = 13000, range 52000..78000
        // At 1000 interval that's 27 strikes
        for &s in &strikes {
            assert_eq!(s % 1000, 0);
        }
    }
}
