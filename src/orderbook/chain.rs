//! Option chain order book module.
//!
//! This module provides the [`OptionChainOrderBook`] and [`OptionChainOrderBookManager`]
//! for managing all strikes within a single expiration.

use super::strike::{StrikeOrderBook, StrikeOrderBookManager};
use crate::error::{Error, Result};
use crossbeam_skiplist::SkipMap;
use optionstratlib::ExpirationDate;
use orderbook_rs::OrderId;
use std::sync::Arc;

/// Option chain order book for a single expiration.
///
/// Contains all strikes for a specific expiration date.
///
/// ## Architecture
///
/// ```text
/// OptionChainOrderBook (per expiration)
///   └── StrikeOrderBookManager
///         └── StrikeOrderBook (per strike)
///               ├── OptionOrderBook (call)
///               └── OptionOrderBook (put)
/// ```
pub struct OptionChainOrderBook {
    /// The underlying asset symbol.
    underlying: String,
    /// The expiration date.
    expiration: ExpirationDate,
    /// Strike order book manager.
    strikes: Arc<StrikeOrderBookManager>,
    /// Unique identifier for this option chain order book.
    id: OrderId,
}

impl OptionChainOrderBook {
    /// Creates a new option chain order book.
    ///
    /// # Arguments
    ///
    /// * `underlying` - The underlying asset symbol (e.g., "BTC")
    /// * `expiration` - The expiration date
    #[must_use]
    pub fn new(underlying: impl Into<String>, expiration: ExpirationDate) -> Self {
        let underlying = underlying.into();

        Self {
            strikes: Arc::new(StrikeOrderBookManager::new(&underlying, expiration)),
            underlying,
            expiration,
            id: OrderId::new(),
        }
    }

    /// Returns the underlying asset symbol.
    #[must_use]
    pub fn underlying(&self) -> &str {
        &self.underlying
    }

    /// Returns the expiration date.
    #[must_use = "returns the expiration date without modifying the book"]
    pub const fn expiration(&self) -> &ExpirationDate {
        &self.expiration
    }

    /// Returns the unique identifier for this option chain order book.
    #[must_use]
    pub const fn id(&self) -> OrderId {
        self.id
    }

    /// Returns a reference to the strike manager.
    #[must_use]
    pub fn strikes(&self) -> &StrikeOrderBookManager {
        &self.strikes
    }

    /// Returns an Arc reference to the strike manager.
    #[must_use]
    pub fn strikes_arc(&self) -> Arc<StrikeOrderBookManager> {
        Arc::clone(&self.strikes)
    }

    /// Gets or creates a strike order book, returning an Arc reference.
    pub fn get_or_create_strike(&self, strike: u64) -> Arc<StrikeOrderBook> {
        self.strikes.get_or_create(strike)
    }

    /// Gets a strike order book.
    ///
    /// # Errors
    ///
    /// Returns `Error::StrikeNotFound` if the strike does not exist.
    pub fn get_strike(&self, strike: u64) -> Result<Arc<StrikeOrderBook>> {
        self.strikes.get(strike)
    }

    /// Returns the number of strikes.
    #[must_use]
    pub fn strike_count(&self) -> usize {
        self.strikes.len()
    }

    /// Returns true if there are no strikes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.strikes.is_empty()
    }

    /// Returns all strike prices (sorted).
    pub fn strike_prices(&self) -> Vec<u64> {
        self.strikes.strike_prices()
    }

    /// Returns the total order count across all strikes.
    #[must_use]
    pub fn total_order_count(&self) -> usize {
        self.strikes.total_order_count()
    }

    /// Returns the ATM strike closest to the given spot price.
    ///
    /// # Errors
    ///
    /// Returns `Error::NoDataAvailable` if there are no strikes.
    pub fn atm_strike(&self, spot: u64) -> Result<u64> {
        self.strikes.atm_strike(spot)
    }

    /// Returns statistics about this option chain.
    #[must_use]
    pub fn stats(&self) -> OptionChainStats {
        OptionChainStats {
            expiration: self.expiration,
            strike_count: self.strike_count(),
            total_orders: self.total_order_count(),
        }
    }
}

/// Statistics about an option chain.
#[derive(Debug, Clone)]
pub struct OptionChainStats {
    /// The expiration date.
    pub expiration: ExpirationDate,
    /// Number of strikes.
    pub strike_count: usize,
    /// Total number of orders.
    pub total_orders: usize,
}

impl std::fmt::Display for OptionChainStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {} strikes, {} orders",
            self.expiration, self.strike_count, self.total_orders
        )
    }
}

/// Manages option chain order books for multiple expirations.
///
/// Uses `SkipMap` for thread-safe concurrent access.
pub struct OptionChainOrderBookManager {
    /// Option chains indexed by expiration.
    chains: SkipMap<ExpirationDate, Arc<OptionChainOrderBook>>,
    /// The underlying asset symbol.
    underlying: String,
}

impl OptionChainOrderBookManager {
    /// Creates a new option chain manager.
    ///
    /// # Arguments
    ///
    /// * `underlying` - The underlying asset symbol
    #[must_use]
    pub fn new(underlying: impl Into<String>) -> Self {
        Self {
            chains: SkipMap::new(),
            underlying: underlying.into(),
        }
    }

    /// Returns the underlying asset symbol.
    #[must_use]
    pub fn underlying(&self) -> &str {
        &self.underlying
    }

    /// Returns the number of option chains.
    #[must_use]
    pub fn len(&self) -> usize {
        self.chains.len()
    }

    /// Returns true if there are no option chains.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chains.is_empty()
    }

    /// Gets or creates an option chain for the given expiration.
    pub fn get_or_create(&self, expiration: ExpirationDate) -> Arc<OptionChainOrderBook> {
        if let Some(entry) = self.chains.get(&expiration) {
            return Arc::clone(entry.value());
        }
        let chain = Arc::new(OptionChainOrderBook::new(&self.underlying, expiration));
        self.chains.insert(expiration, Arc::clone(&chain));
        chain
    }

    /// Gets an option chain by expiration.
    ///
    /// # Errors
    ///
    /// Returns `Error::ExpirationNotFound` if the expiration does not exist.
    pub fn get(&self, expiration: &ExpirationDate) -> Result<Arc<OptionChainOrderBook>> {
        self.chains
            .get(expiration)
            .map(|e| Arc::clone(e.value()))
            .ok_or_else(|| Error::expiration_not_found(expiration.to_string()))
    }

    /// Returns true if an option chain exists for the expiration.
    #[must_use]
    pub fn contains(&self, expiration: &ExpirationDate) -> bool {
        self.chains.contains_key(expiration)
    }

    /// Returns an iterator over all chains.
    pub fn iter(
        &self,
    ) -> impl Iterator<
        Item = crossbeam_skiplist::map::Entry<'_, ExpirationDate, Arc<OptionChainOrderBook>>,
    > {
        self.chains.iter()
    }

    /// Removes an option chain.
    pub fn remove(&self, expiration: &ExpirationDate) -> bool {
        self.chains.remove(expiration).is_some()
    }

    /// Returns the total order count across all chains.
    #[must_use]
    pub fn total_order_count(&self) -> usize {
        self.chains
            .iter()
            .map(|e| e.value().total_order_count())
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use optionstratlib::prelude::pos_or_panic;
    use orderbook_rs::{OrderId, Side};

    fn test_expiration() -> ExpirationDate {
        ExpirationDate::Days(pos_or_panic!(30.0))
    }

    #[test]
    fn test_option_chain_creation() {
        let chain = OptionChainOrderBook::new("BTC", test_expiration());

        assert_eq!(chain.underlying(), "BTC");
        assert!(chain.is_empty());
    }

    #[test]
    fn test_option_chain_strikes() {
        let chain = OptionChainOrderBook::new("BTC", test_expiration());

        drop(chain.get_or_create_strike(50000));
        drop(chain.get_or_create_strike(55000));
        drop(chain.get_or_create_strike(45000));

        assert_eq!(chain.strike_count(), 3);
        assert_eq!(chain.strike_prices(), vec![45000, 50000, 55000]);
    }

    #[test]
    fn test_option_chain_orders() {
        let chain = OptionChainOrderBook::new("BTC", test_expiration());

        {
            let strike = chain.get_or_create_strike(50000);
            strike
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 100, 10)
                .unwrap();
            strike
                .put()
                .add_limit_order(OrderId::new(), Side::Sell, 50, 5)
                .unwrap();
        }

        assert_eq!(chain.total_order_count(), 2);
    }

    #[test]
    fn test_option_chain_stats() {
        let chain = OptionChainOrderBook::new("BTC", test_expiration());

        {
            let strike = chain.get_or_create_strike(50000);
            strike
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 100, 10)
                .unwrap();
            strike
                .call()
                .add_limit_order(OrderId::new(), Side::Sell, 101, 5)
                .unwrap();
            strike
                .put()
                .add_limit_order(OrderId::new(), Side::Buy, 50, 10)
                .unwrap();
            strike
                .put()
                .add_limit_order(OrderId::new(), Side::Sell, 51, 5)
                .unwrap();
        }

        let stats = chain.stats();
        assert_eq!(stats.strike_count, 1);
        assert_eq!(stats.total_orders, 4);
    }

    #[test]
    fn test_option_chain_manager() {
        let manager = OptionChainOrderBookManager::new("BTC");

        drop(manager.get_or_create(ExpirationDate::Days(pos_or_panic!(30.0))));
        drop(manager.get_or_create(ExpirationDate::Days(pos_or_panic!(60.0))));
        drop(manager.get_or_create(ExpirationDate::Days(pos_or_panic!(90.0))));

        assert_eq!(manager.len(), 3);
    }

    #[test]
    fn test_option_chain_expiration() {
        let exp = test_expiration();
        let chain = OptionChainOrderBook::new("BTC", exp);
        assert_eq!(*chain.expiration(), exp);
    }

    #[test]
    fn test_option_chain_strikes_ref() {
        let chain = OptionChainOrderBook::new("BTC", test_expiration());
        drop(chain.get_or_create_strike(50000));
        let strikes = chain.strikes();
        assert_eq!(strikes.len(), 1);
    }

    #[test]
    fn test_option_chain_get_strike() {
        let chain = OptionChainOrderBook::new("BTC", test_expiration());
        drop(chain.get_or_create_strike(50000));

        assert!(chain.get_strike(50000).is_ok());
        assert!(chain.get_strike(99999).is_err());
    }

    #[test]
    fn test_option_chain_atm_strike() {
        let chain = OptionChainOrderBook::new("BTC", test_expiration());

        drop(chain.get_or_create_strike(45000));
        drop(chain.get_or_create_strike(50000));
        drop(chain.get_or_create_strike(55000));

        assert_eq!(chain.atm_strike(48000).unwrap(), 50000);
        assert_eq!(chain.atm_strike(53000).unwrap(), 55000);
    }

    #[test]
    fn test_option_chain_atm_strike_empty() {
        let chain = OptionChainOrderBook::new("BTC", test_expiration());
        assert!(chain.atm_strike(50000).is_err());
    }

    #[test]
    fn test_option_chain_stats_display() {
        let chain = OptionChainOrderBook::new("BTC", test_expiration());
        drop(chain.get_or_create_strike(50000));

        let stats = chain.stats();
        let display = format!("{}", stats);
        assert!(display.contains("1 strikes"));
    }

    #[test]
    fn test_option_chain_manager_underlying() {
        let manager = OptionChainOrderBookManager::new("BTC");
        assert_eq!(manager.underlying(), "BTC");
    }

    #[test]
    fn test_option_chain_manager_is_empty() {
        let manager = OptionChainOrderBookManager::new("BTC");
        assert!(manager.is_empty());

        drop(manager.get_or_create(test_expiration()));
        assert!(!manager.is_empty());
    }

    #[test]
    fn test_option_chain_manager_get() {
        let manager = OptionChainOrderBookManager::new("BTC");
        let exp = test_expiration();

        drop(manager.get_or_create(exp));

        assert!(manager.get(&exp).is_ok());
        assert!(
            manager
                .get(&ExpirationDate::Days(pos_or_panic!(999.0)))
                .is_err()
        );
    }

    #[test]
    fn test_option_chain_manager_contains() {
        let manager = OptionChainOrderBookManager::new("BTC");
        let exp = test_expiration();

        drop(manager.get_or_create(exp));

        assert!(manager.contains(&exp));
        assert!(!manager.contains(&ExpirationDate::Days(pos_or_panic!(999.0))));
    }

    #[test]
    fn test_option_chain_manager_remove() {
        let manager = OptionChainOrderBookManager::new("BTC");
        let exp = test_expiration();

        drop(manager.get_or_create(exp));
        assert_eq!(manager.len(), 1);

        assert!(manager.remove(&exp));
        assert_eq!(manager.len(), 0);
        assert!(!manager.remove(&exp));
    }

    #[test]
    fn test_option_chain_manager_total_order_count() {
        let manager = OptionChainOrderBookManager::new("BTC");

        let chain = manager.get_or_create(test_expiration());
        let strike = chain.get_or_create_strike(50000);
        strike
            .call()
            .add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();
        drop(strike);
        drop(chain);

        assert_eq!(manager.total_order_count(), 1);
    }
}
