//! Underlying order book module.
//!
//! This module provides the [`UnderlyingOrderBook`] and [`UnderlyingOrderBookManager`]
//! for managing all underlyings in the system.

use super::contract_specs::{ContractSpecs, SharedContractSpecs};
use super::expiration::{ExpirationOrderBook, ExpirationOrderBookManager};
use super::validation::ValidationConfig;
use crate::error::{Error, Result};
use crossbeam_skiplist::SkipMap;
use optionstratlib::ExpirationDate;
use std::sync::Arc;

/// Order book for a single underlying asset.
///
/// Contains all expirations for a specific underlying.
///
/// ## Architecture
///
/// ```text
/// UnderlyingOrderBook (per underlying)
///   └── ExpirationOrderBookManager
///         └── ExpirationOrderBook (per expiry)
///               └── OptionChainOrderBook
///                     └── StrikeOrderBook (per strike)
/// ```
pub struct UnderlyingOrderBook {
    /// The underlying asset symbol.
    underlying: String,
    /// Expiration order book manager.
    expirations: ExpirationOrderBookManager,
    /// Contract specifications for this underlying.
    specs: SharedContractSpecs,
}

impl UnderlyingOrderBook {
    /// Creates a new underlying order book.
    ///
    /// # Arguments
    ///
    /// * `underlying` - The underlying asset symbol (e.g., "BTC")
    #[must_use]
    pub fn new(underlying: impl Into<String>) -> Self {
        let underlying = underlying.into();

        Self {
            expirations: ExpirationOrderBookManager::new(&underlying),
            underlying,
            specs: SharedContractSpecs::new(),
        }
    }

    /// Returns the underlying asset symbol.
    #[must_use]
    pub fn underlying(&self) -> &str {
        &self.underlying
    }

    /// Returns a reference to the expiration manager.
    #[must_use]
    pub const fn expirations(&self) -> &ExpirationOrderBookManager {
        &self.expirations
    }

    /// Sets the contract specifications for this underlying.
    ///
    /// Automatically derives and applies a [`ValidationConfig`] from the specs'
    /// tick size, lot size, min/max order size fields. This validation config
    /// is propagated to all future expirations and strikes.
    ///
    /// Existing expiration books and strikes are not affected by the derived
    /// validation config.
    pub fn set_specs(&self, specs: ContractSpecs) {
        let validation = specs.to_validation_config();
        self.expirations.set_specs(specs.clone());
        self.specs.set(specs);
        self.expirations.set_validation(validation);
    }

    /// Returns the current contract specifications, if any.
    #[must_use]
    pub fn specs(&self) -> Option<ContractSpecs> {
        self.specs.get()
    }

    /// Sets the validation config for all future expirations and strikes
    /// created within this underlying.
    ///
    /// Delegates to [`ExpirationOrderBookManager::set_validation`].
    /// Existing expiration books and strikes are not affected.
    pub fn set_validation(&self, config: ValidationConfig) {
        self.expirations.set_validation(config);
    }

    /// Returns the current validation config, if any.
    #[must_use]
    pub fn validation_config(&self) -> Option<ValidationConfig> {
        self.expirations.validation_config()
    }

    /// Gets or creates an expiration order book, returning an Arc reference.
    pub fn get_or_create_expiration(&self, expiration: ExpirationDate) -> Arc<ExpirationOrderBook> {
        self.expirations.get_or_create(expiration)
    }

    /// Gets an expiration order book.
    ///
    /// # Errors
    ///
    /// Returns `Error::ExpirationNotFound` if the expiration does not exist.
    pub fn get_expiration(&self, expiration: &ExpirationDate) -> Result<Arc<ExpirationOrderBook>> {
        self.expirations.get(expiration)
    }

    /// Returns the number of expirations.
    #[must_use]
    pub fn expiration_count(&self) -> usize {
        self.expirations.len()
    }

    /// Returns true if there are no expirations.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.expirations.is_empty()
    }

    /// Returns the total order count across all expirations.
    #[must_use]
    pub fn total_order_count(&self) -> usize {
        self.expirations.total_order_count()
    }

    /// Returns the total strike count across all expirations.
    #[must_use]
    pub fn total_strike_count(&self) -> usize {
        self.expirations.total_strike_count()
    }

    /// Returns statistics about this underlying.
    #[must_use]
    pub fn stats(&self) -> UnderlyingStats {
        UnderlyingStats {
            underlying: self.underlying.clone(),
            expiration_count: self.expiration_count(),
            total_strikes: self.total_strike_count(),
            total_orders: self.total_order_count(),
        }
    }
}

/// Statistics about an underlying order book.
#[derive(Debug, Clone)]
pub struct UnderlyingStats {
    /// The underlying asset symbol.
    pub underlying: String,
    /// Number of expirations.
    pub expiration_count: usize,
    /// Total number of strikes.
    pub total_strikes: usize,
    /// Total number of orders.
    pub total_orders: usize,
}

impl std::fmt::Display for UnderlyingStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {} expirations, {} strikes, {} orders",
            self.underlying, self.expiration_count, self.total_strikes, self.total_orders
        )
    }
}

/// Manages underlying order books for all assets.
///
/// This is the top-level manager for the entire order book hierarchy.
/// Uses `SkipMap` for thread-safe concurrent access.
///
/// ## Architecture
///
/// ```text
/// UnderlyingOrderBookManager (root)
///   └── UnderlyingOrderBook (per underlying: BTC, ETH, SPX, etc.)
///         └── ExpirationOrderBookManager
///               └── ExpirationOrderBook (per expiry)
///                     └── OptionChainOrderBook
///                           └── StrikeOrderBook (per strike)
///                                 ├── OptionOrderBook (call)
///                                 └── OptionOrderBook (put)
/// ```
pub struct UnderlyingOrderBookManager {
    /// Underlying order books indexed by symbol.
    underlyings: SkipMap<String, Arc<UnderlyingOrderBook>>,
}

impl Default for UnderlyingOrderBookManager {
    fn default() -> Self {
        Self::new()
    }
}

impl UnderlyingOrderBookManager {
    /// Creates a new underlying order book manager.
    #[must_use]
    pub fn new() -> Self {
        Self {
            underlyings: SkipMap::new(),
        }
    }

    /// Returns the number of underlyings.
    #[must_use]
    pub fn len(&self) -> usize {
        self.underlyings.len()
    }

    /// Returns true if there are no underlyings.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.underlyings.is_empty()
    }

    /// Gets or creates an underlying order book.
    pub fn get_or_create(&self, underlying: impl Into<String>) -> Arc<UnderlyingOrderBook> {
        let underlying = underlying.into();
        if let Some(entry) = self.underlyings.get(&underlying) {
            return Arc::clone(entry.value());
        }
        let book = Arc::new(UnderlyingOrderBook::new(&underlying));
        self.underlyings.insert(underlying, Arc::clone(&book));
        book
    }

    /// Gets an underlying order book.
    ///
    /// # Errors
    ///
    /// Returns `Error::UnderlyingNotFound` if the underlying does not exist.
    pub fn get(&self, underlying: &str) -> Result<Arc<UnderlyingOrderBook>> {
        self.underlyings
            .get(underlying)
            .map(|e| Arc::clone(e.value()))
            .ok_or_else(|| Error::underlying_not_found(underlying))
    }

    /// Returns true if an underlying exists.
    #[must_use]
    pub fn contains(&self, underlying: &str) -> bool {
        self.underlyings.contains_key(underlying)
    }

    /// Returns an iterator over all underlyings.
    pub fn iter(
        &self,
    ) -> impl Iterator<Item = crossbeam_skiplist::map::Entry<'_, String, Arc<UnderlyingOrderBook>>>
    {
        self.underlyings.iter()
    }

    /// Removes an underlying order book.
    pub fn remove(&self, underlying: &str) -> bool {
        self.underlyings.remove(underlying).is_some()
    }

    /// Returns all underlying symbols (sorted).
    /// SkipMap maintains sorted order, so no additional sorting needed.
    pub fn underlying_symbols(&self) -> Vec<String> {
        self.underlyings.iter().map(|e| e.key().clone()).collect()
    }

    /// Returns the total order count across all underlyings.
    #[must_use]
    pub fn total_order_count(&self) -> usize {
        self.underlyings
            .iter()
            .map(|e| e.value().total_order_count())
            .sum()
    }

    /// Returns the total expiration count across all underlyings.
    #[must_use]
    pub fn total_expiration_count(&self) -> usize {
        self.underlyings
            .iter()
            .map(|e| e.value().expiration_count())
            .sum()
    }

    /// Returns the total strike count across all underlyings.
    #[must_use]
    pub fn total_strike_count(&self) -> usize {
        self.underlyings
            .iter()
            .map(|e| e.value().total_strike_count())
            .sum()
    }

    /// Returns statistics about the entire order book system.
    #[must_use]
    pub fn stats(&self) -> GlobalStats {
        GlobalStats {
            underlying_count: self.len(),
            total_expirations: self.total_expiration_count(),
            total_strikes: self.total_strike_count(),
            total_orders: self.total_order_count(),
        }
    }
}

/// Global statistics about the order book system.
#[derive(Debug, Clone)]
pub struct GlobalStats {
    /// Number of underlyings.
    pub underlying_count: usize,
    /// Total number of expirations.
    pub total_expirations: usize,
    /// Total number of strikes.
    pub total_strikes: usize,
    /// Total number of orders.
    pub total_orders: usize,
}

impl std::fmt::Display for GlobalStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} underlyings, {} expirations, {} strikes, {} orders",
            self.underlying_count, self.total_expirations, self.total_strikes, self.total_orders
        )
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
    fn test_underlying_order_book_creation() {
        let book = UnderlyingOrderBook::new("BTC");

        assert_eq!(book.underlying(), "BTC");
        assert!(book.is_empty());
    }

    #[test]
    fn test_underlying_order_book_hierarchy() {
        let book = UnderlyingOrderBook::new("BTC");

        let exp = book.get_or_create_expiration(test_expiration());
        let strike = exp.get_or_create_strike(50000);
        strike
            .call()
            .add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();

        assert_eq!(book.expiration_count(), 1);
        assert_eq!(book.total_strike_count(), 1);
        assert_eq!(book.total_order_count(), 1);
    }

    #[test]
    fn test_underlying_order_book_get_expiration() {
        let book = UnderlyingOrderBook::new("BTC");
        let exp_date = test_expiration();

        book.get_or_create_expiration(exp_date);

        let exp = book.get_expiration(&exp_date);
        assert!(exp.is_ok());

        let missing_exp = ExpirationDate::Days(pos_or_panic!(90.0));
        let missing = book.get_expiration(&missing_exp);
        assert!(missing.is_err());
    }

    #[test]
    fn test_underlying_manager_creation() {
        let manager = UnderlyingOrderBookManager::new();

        assert!(manager.is_empty());
        assert_eq!(manager.len(), 0);
    }

    #[test]
    fn test_underlying_manager_get_or_create() {
        let manager = UnderlyingOrderBookManager::new();

        drop(manager.get_or_create("BTC"));
        drop(manager.get_or_create("ETH"));
        drop(manager.get_or_create("SPX"));

        assert_eq!(manager.len(), 3);
    }

    #[test]
    fn test_underlying_manager_full_hierarchy() {
        let manager = UnderlyingOrderBookManager::new();
        let exp_date = test_expiration();

        // Create BTC chain
        {
            let btc = manager.get_or_create("BTC");
            let exp = btc.get_or_create_expiration(exp_date);
            let strike = exp.get_or_create_strike(50000);
            strike
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 100, 10)
                .unwrap();
            strike
                .put()
                .add_limit_order(OrderId::new(), Side::Sell, 50, 5)
                .unwrap();
        }

        // Create ETH chain
        {
            let eth = manager.get_or_create("ETH");
            let exp = eth.get_or_create_expiration(exp_date);
            exp.get_or_create_strike(3000);
        }

        let stats = manager.stats();
        assert_eq!(stats.underlying_count, 2);
        assert_eq!(stats.total_expirations, 2);
        assert_eq!(stats.total_strikes, 2);
        assert_eq!(stats.total_orders, 2);
    }

    #[test]
    fn test_underlying_order_book_expirations() {
        let book = UnderlyingOrderBook::new("BTC");
        drop(book.get_or_create_expiration(test_expiration()));
        let expirations = book.expirations();
        assert_eq!(expirations.len(), 1);
    }

    #[test]
    fn test_underlying_order_book_stats() {
        let book = UnderlyingOrderBook::new("BTC");

        let exp = book.get_or_create_expiration(test_expiration());
        let strike = exp.get_or_create_strike(50000);
        strike
            .call()
            .add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();
        drop(strike);
        drop(exp);

        let stats = book.stats();
        assert_eq!(stats.underlying, "BTC");
        assert_eq!(stats.expiration_count, 1);
        assert_eq!(stats.total_strikes, 1);
        assert_eq!(stats.total_orders, 1);

        let display = format!("{}", stats);
        assert!(display.contains("BTC"));
    }

    #[test]
    fn test_underlying_manager_get() {
        let manager = UnderlyingOrderBookManager::new();

        drop(manager.get_or_create("BTC"));

        assert!(manager.get("BTC").is_ok());
        assert!(manager.get("XRP").is_err());
    }

    #[test]
    fn test_underlying_manager_contains() {
        let manager = UnderlyingOrderBookManager::new();

        drop(manager.get_or_create("BTC"));

        assert!(manager.contains("BTC"));
        assert!(!manager.contains("XRP"));
    }

    #[test]
    fn test_underlying_manager_remove() {
        let manager = UnderlyingOrderBookManager::new();

        drop(manager.get_or_create("BTC"));
        drop(manager.get_or_create("ETH"));

        assert_eq!(manager.len(), 2);
        assert!(manager.remove("BTC"));
        assert_eq!(manager.len(), 1);
        assert!(!manager.remove("BTC"));
    }

    #[test]
    fn test_underlying_manager_underlying_symbols() {
        let manager = UnderlyingOrderBookManager::new();

        drop(manager.get_or_create("BTC"));
        drop(manager.get_or_create("ETH"));
        drop(manager.get_or_create("SPX"));

        let symbols = manager.underlying_symbols();
        assert_eq!(symbols.len(), 3);
        assert!(symbols.contains(&"BTC".to_string()));
        assert!(symbols.contains(&"ETH".to_string()));
        assert!(symbols.contains(&"SPX".to_string()));
    }

    #[test]
    fn test_underlying_manager_total_order_count() {
        let manager = UnderlyingOrderBookManager::new();

        let btc = manager.get_or_create("BTC");
        let exp = btc.get_or_create_expiration(test_expiration());
        let strike = exp.get_or_create_strike(50000);
        strike
            .call()
            .add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();
        drop(strike);
        drop(exp);
        drop(btc);

        assert_eq!(manager.total_order_count(), 1);
    }

    #[test]
    fn test_global_stats_display() {
        let manager = UnderlyingOrderBookManager::new();

        let btc = manager.get_or_create("BTC");
        let exp = btc.get_or_create_expiration(test_expiration());
        exp.get_or_create_strike(50000);
        drop(exp);
        drop(btc);

        let stats = manager.stats();
        let display = format!("{}", stats);
        assert!(display.contains("1 underlyings"));
        assert!(display.contains("1 expirations"));
        assert!(display.contains("1 strikes"));
    }

    #[test]
    fn test_underlying_set_validation() {
        let book = UnderlyingOrderBook::new("BTC");
        let config = ValidationConfig::new().with_tick_size(100);
        book.set_validation(config.clone());

        assert_eq!(book.validation_config(), Some(config));

        let exp = book.get_or_create_expiration(test_expiration());
        let strike = exp.get_or_create_strike(50000);
        assert!(
            strike
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 200, 10)
                .is_ok()
        );
        assert!(
            strike
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 150, 10)
                .is_err()
        );
    }

    #[test]
    fn test_underlying_set_validation_full_hierarchy() {
        let manager = UnderlyingOrderBookManager::new();
        let btc = manager.get_or_create("BTC");

        let config = ValidationConfig::new()
            .with_tick_size(100)
            .with_lot_size(10)
            .with_min_order_size(5)
            .with_max_order_size(1000);
        btc.set_validation(config);

        let exp = btc.get_or_create_expiration(test_expiration());
        let strike = exp.get_or_create_strike(50000);

        // Valid: price=200 (tick 100), qty=20 (lot 10, range 5..1000)
        assert!(
            strike
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 200, 20)
                .is_ok()
        );

        // Invalid tick
        assert!(
            strike
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 150, 20)
                .is_err()
        );

        // Invalid lot
        assert!(
            strike
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 200, 15)
                .is_err()
        );

        // Too small
        assert!(
            strike
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 200, 2)
                .is_err()
        );

        // Too large
        assert!(
            strike
                .put()
                .add_limit_order(OrderId::new(), Side::Buy, 200, 2000)
                .is_err()
        );
    }

    #[test]
    fn test_underlying_no_validation_by_default() {
        let book = UnderlyingOrderBook::new("BTC");
        assert!(book.validation_config().is_none());

        let exp = book.get_or_create_expiration(test_expiration());
        let strike = exp.get_or_create_strike(50000);
        assert!(
            strike
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 150, 7)
                .is_ok()
        );
    }

    #[test]
    fn test_underlying_no_specs_by_default() {
        let book = UnderlyingOrderBook::new("BTC");
        assert!(book.specs().is_none());
    }

    #[test]
    fn test_underlying_set_specs() {
        use crate::orderbook::contract_specs::{ContractSpecs, ExerciseStyle, SettlementType};

        let book = UnderlyingOrderBook::new("BTC");
        let specs = ContractSpecs::builder()
            .tick_size(100)
            .lot_size(10)
            .contract_size(1)
            .min_order_size(5)
            .max_order_size(1000)
            .settlement(SettlementType::Cash)
            .exercise_style(ExerciseStyle::European)
            .settlement_currency("USDC")
            .build();

        book.set_specs(specs.clone());

        assert_eq!(book.specs(), Some(specs));
    }

    #[test]
    fn test_underlying_set_specs_derives_validation() {
        use crate::orderbook::contract_specs::ContractSpecs;

        let book = UnderlyingOrderBook::new("BTC");
        let specs = ContractSpecs::builder()
            .tick_size(100)
            .lot_size(10)
            .min_order_size(5)
            .max_order_size(1000)
            .build();

        book.set_specs(specs);

        // Validation should be auto-derived
        let config = book.validation_config();
        assert!(config.is_some());
        let config = config.unwrap();
        assert_eq!(config.tick_size(), Some(100));
        assert_eq!(config.lot_size(), Some(10));
        assert_eq!(config.min_order_size(), Some(5));
        assert_eq!(config.max_order_size(), Some(1000));
    }

    #[test]
    fn test_underlying_set_specs_enforces_validation_on_new_strikes() {
        use crate::orderbook::contract_specs::ContractSpecs;

        let book = UnderlyingOrderBook::new("BTC");
        let specs = ContractSpecs::builder()
            .tick_size(100)
            .lot_size(10)
            .min_order_size(10)
            .max_order_size(1000)
            .build();

        book.set_specs(specs);

        let exp = book.get_or_create_expiration(test_expiration());
        let strike = exp.get_or_create_strike(50000);

        // Valid: price=200 (tick 100), qty=20 (lot 10, range 10..1000)
        assert!(
            strike
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 200, 20)
                .is_ok()
        );

        // Invalid tick
        assert!(
            strike
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 150, 20)
                .is_err()
        );

        // Invalid lot
        assert!(
            strike
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 200, 15)
                .is_err()
        );

        // Too small
        assert!(
            strike
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 200, 5)
                .is_err()
        );

        // Too large
        assert!(
            strike
                .put()
                .add_limit_order(OrderId::new(), Side::Buy, 200, 2000)
                .is_err()
        );
    }

    #[test]
    fn test_underlying_specs_propagate_through_full_hierarchy() {
        use crate::orderbook::contract_specs::{ContractSpecs, ExerciseStyle, SettlementType};

        let manager = UnderlyingOrderBookManager::new();
        let btc = manager.get_or_create("BTC");

        let specs = ContractSpecs::builder()
            .tick_size(100)
            .lot_size(10)
            .contract_size(1)
            .min_order_size(10)
            .max_order_size(1000)
            .settlement(SettlementType::Cash)
            .exercise_style(ExerciseStyle::European)
            .settlement_currency("USDC")
            .build();

        btc.set_specs(specs.clone());

        // Create expiration after specs are set
        let exp = btc.get_or_create_expiration(test_expiration());

        // Specs should be accessible from expiration
        assert_eq!(exp.specs(), Some(specs.clone()));

        // Specs should be accessible from chain
        assert_eq!(exp.chain().specs(), Some(specs.clone()));

        // Validation should enforce tick size on new strikes
        let strike = exp.get_or_create_strike(50000);
        assert!(
            strike
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 200, 20)
                .is_ok()
        );
        assert!(
            strike
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 150, 20)
                .is_err()
        );
    }

    #[test]
    fn test_underlying_specs_existing_expiration_unaffected() {
        use crate::orderbook::contract_specs::ContractSpecs;

        let book = UnderlyingOrderBook::new("BTC");

        // Create expiration BEFORE setting specs
        let exp_before = book.get_or_create_expiration(ExpirationDate::Days(pos_or_panic!(30.0)));

        // Set specs after
        book.set_specs(ContractSpecs::builder().tick_size(100).build());

        // Existing expiration's new strikes are NOT affected by validation
        let strike = exp_before.get_or_create_strike(50000);
        assert!(
            strike
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 150, 7)
                .is_ok()
        );

        // New expiration IS affected
        let exp_after = book.get_or_create_expiration(ExpirationDate::Days(pos_or_panic!(60.0)));
        let strike2 = exp_after.get_or_create_strike(50000);
        assert!(
            strike2
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 150, 7)
                .is_err()
        );
    }

    #[test]
    fn test_underlying_default_specs_are_permissive() {
        use crate::orderbook::contract_specs::ContractSpecs;

        let book = UnderlyingOrderBook::new("BTC");
        book.set_specs(ContractSpecs::default());

        let exp = book.get_or_create_expiration(test_expiration());
        let strike = exp.get_or_create_strike(50000);

        // Default specs: tick=1, lot=1, min=1, max=u64::MAX → everything passes
        assert!(
            strike
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 150, 7)
                .is_ok()
        );
    }
}
