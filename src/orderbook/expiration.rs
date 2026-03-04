//! Expiration order book module.
//!
//! This module provides the [`ExpirationOrderBook`] and [`ExpirationOrderBookManager`]
//! for managing all expirations for a single underlying asset.

use super::chain::OptionChainOrderBook;
use super::contract_specs::{ContractSpecs, SharedContractSpecs};
use super::instrument_registry::InstrumentRegistry;
use super::stp::SharedSTPMode;
use super::strike::StrikeOrderBook;
use super::validation::{SharedValidationConfig, ValidationConfig};
use crate::error::{Error, Result};
use crossbeam_skiplist::SkipMap;
use optionstratlib::ExpirationDate;
use orderbook_rs::{OrderId, STPMode};
use std::sync::Arc;

/// Order book for a single expiration date.
///
/// Contains the complete option chain for a specific expiration.
///
/// ## Architecture
///
/// ```text
/// ExpirationOrderBook (per expiry date)
///   └── OptionChainOrderBook
///         └── StrikeOrderBookManager
///               └── StrikeOrderBook (per strike)
/// ```
pub struct ExpirationOrderBook {
    /// The underlying asset symbol.
    underlying: String,
    /// The expiration date.
    expiration: ExpirationDate,
    /// The option chain for this expiration.
    chain: Arc<OptionChainOrderBook>,
    /// Unique identifier for this expiration order book.
    id: OrderId,
    /// Instrument registry propagated to the chain.
    /// Stored to keep the `Arc` reference alive for the hierarchy.
    #[allow(dead_code)]
    registry: Option<Arc<InstrumentRegistry>>,
}

impl ExpirationOrderBook {
    /// Creates a new expiration order book.
    ///
    /// # Arguments
    ///
    /// * `underlying` - The underlying asset symbol (e.g., "BTC")
    /// * `expiration` - The expiration date
    #[must_use]
    pub fn new(underlying: impl Into<String>, expiration: ExpirationDate) -> Self {
        let underlying = underlying.into();

        Self {
            chain: Arc::new(OptionChainOrderBook::new(&underlying, expiration)),
            underlying,
            expiration,
            id: OrderId::new(),
            registry: None,
        }
    }

    /// Creates a new expiration order book with an instrument registry.
    ///
    /// The registry is propagated to the internal [`OptionChainOrderBook`]
    /// and its strike manager.
    ///
    /// # Arguments
    ///
    /// * `underlying` - The underlying asset symbol
    /// * `expiration` - The expiration date
    /// * `registry` - The instrument registry for ID allocation
    #[must_use]
    pub(crate) fn new_with_registry(
        underlying: impl Into<String>,
        expiration: ExpirationDate,
        registry: Arc<InstrumentRegistry>,
    ) -> Self {
        let underlying = underlying.into();

        Self {
            chain: Arc::new(OptionChainOrderBook::new_with_registry(
                &underlying,
                expiration,
                Arc::clone(&registry),
            )),
            underlying,
            expiration,
            id: OrderId::new(),
            registry: Some(registry),
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

    /// Returns the unique identifier for this expiration order book.
    #[must_use]
    pub const fn id(&self) -> OrderId {
        self.id
    }

    /// Returns a reference to the option chain.
    #[must_use]
    pub fn chain(&self) -> &OptionChainOrderBook {
        &self.chain
    }

    /// Returns an Arc reference to the option chain.
    #[must_use]
    pub fn chain_arc(&self) -> Arc<OptionChainOrderBook> {
        Arc::clone(&self.chain)
    }

    /// Returns the contract specifications, if any.
    ///
    /// Delegates to the underlying [`OptionChainOrderBook::specs`].
    #[must_use]
    pub fn specs(&self) -> Option<ContractSpecs> {
        self.chain.specs()
    }

    /// Sets the validation config for all future strikes created within this expiration.
    ///
    /// Delegates to the underlying [`OptionChainOrderBook::set_validation`].
    /// Existing strikes are not affected.
    pub fn set_validation(&self, config: ValidationConfig) {
        self.chain.set_validation(config);
    }

    /// Returns the current validation config, if any.
    #[must_use]
    pub fn validation_config(&self) -> Option<ValidationConfig> {
        self.chain.validation_config()
    }

    /// Sets the STP mode for all future option books created within this expiration.
    ///
    /// Delegates to the underlying [`OptionChainOrderBook::set_stp_mode`].
    /// Existing books are not affected.
    pub fn set_stp_mode(&self, mode: STPMode) {
        self.chain.set_stp_mode(mode);
    }

    /// Returns the current STP mode.
    #[must_use]
    pub fn stp_mode(&self) -> STPMode {
        self.chain.stp_mode()
    }

    /// Gets or creates a strike order book, returning an Arc reference.
    pub fn get_or_create_strike(&self, strike: u64) -> Arc<StrikeOrderBook> {
        self.chain.get_or_create_strike(strike)
    }

    /// Gets a strike order book.
    ///
    /// # Errors
    ///
    /// Returns `Error::StrikeNotFound` if the strike does not exist.
    pub fn get_strike(&self, strike: u64) -> Result<Arc<StrikeOrderBook>> {
        self.chain.get_strike(strike)
    }

    /// Returns the number of strikes.
    #[must_use]
    pub fn strike_count(&self) -> usize {
        self.chain.strike_count()
    }

    /// Returns true if there are no strikes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chain.is_empty()
    }

    /// Returns all strike prices (sorted).
    pub fn strike_prices(&self) -> Vec<u64> {
        self.chain.strike_prices()
    }

    /// Returns the total order count.
    #[must_use]
    pub fn total_order_count(&self) -> usize {
        self.chain.total_order_count()
    }

    /// Returns the ATM strike closest to the given spot price.
    ///
    /// # Errors
    ///
    /// Returns `Error::NoDataAvailable` if there are no strikes.
    pub fn atm_strike(&self, spot: u64) -> Result<u64> {
        self.chain.atm_strike(spot)
    }
}

/// Manages expiration order books for a single underlying.
///
/// Provides centralized access to all expirations for an underlying asset.
/// Uses `SkipMap` for thread-safe concurrent access.
pub struct ExpirationOrderBookManager {
    /// Expiration order books indexed by expiration date.
    expirations: SkipMap<ExpirationDate, Arc<ExpirationOrderBook>>,
    /// The underlying asset symbol.
    underlying: String,
    /// Validation config applied to newly created expiration books.
    validation_config: SharedValidationConfig,
    /// Contract specs propagated to newly created expiration books.
    contract_specs: SharedContractSpecs,
    /// Instrument registry propagated to newly created expiration books.
    registry: Option<Arc<InstrumentRegistry>>,
    /// STP mode propagated to newly created expiration books.
    stp_mode: SharedSTPMode,
}

impl ExpirationOrderBookManager {
    /// Creates a new expiration order book manager.
    ///
    /// # Arguments
    ///
    /// * `underlying` - The underlying asset symbol
    #[must_use]
    pub fn new(underlying: impl Into<String>) -> Self {
        Self {
            expirations: SkipMap::new(),
            underlying: underlying.into(),
            validation_config: SharedValidationConfig::new(),
            contract_specs: SharedContractSpecs::new(),
            registry: None,
            stp_mode: SharedSTPMode::new(),
        }
    }

    /// Creates a new expiration order book manager with an instrument registry.
    ///
    /// The registry is propagated to newly created expiration books and
    /// their chains.
    ///
    /// # Arguments
    ///
    /// * `underlying` - The underlying asset symbol
    /// * `registry` - The instrument registry for ID allocation
    #[must_use]
    pub(crate) fn new_with_registry(
        underlying: impl Into<String>,
        registry: Arc<InstrumentRegistry>,
    ) -> Self {
        Self {
            expirations: SkipMap::new(),
            underlying: underlying.into(),
            validation_config: SharedValidationConfig::new(),
            contract_specs: SharedContractSpecs::new(),
            registry: Some(registry),
            stp_mode: SharedSTPMode::new(),
        }
    }

    /// Sets the contract specs for all future expirations created by this manager.
    ///
    /// Existing expiration books are not affected. Only newly created books
    /// via [`get_or_create`](Self::get_or_create) will have these specs propagated.
    pub fn set_specs(&self, specs: ContractSpecs) {
        self.contract_specs.set(specs);
    }

    /// Returns the current contract specs, if any.
    #[must_use]
    pub fn specs(&self) -> Option<ContractSpecs> {
        self.contract_specs.get()
    }

    /// Sets the validation config for all future expirations created by this manager.
    ///
    /// Existing expiration books are not affected. Only newly created books
    /// via [`get_or_create`](Self::get_or_create) will have this config applied.
    pub fn set_validation(&self, config: ValidationConfig) {
        self.validation_config.set(config);
    }

    /// Returns the current validation config, if any.
    #[must_use]
    pub fn validation_config(&self) -> Option<ValidationConfig> {
        self.validation_config.get()
    }

    /// Sets the STP mode for all future expiration books created by this manager.
    ///
    /// Existing books are not affected. Only newly created books
    /// via [`get_or_create`](Self::get_or_create) will have this mode propagated.
    pub fn set_stp_mode(&self, mode: STPMode) {
        self.stp_mode.set(mode);
    }

    /// Returns the current STP mode.
    #[must_use]
    pub fn stp_mode(&self) -> STPMode {
        self.stp_mode.get()
    }

    /// Returns the underlying asset symbol.
    #[must_use]
    pub fn underlying(&self) -> &str {
        &self.underlying
    }

    /// Returns the number of expirations.
    #[must_use]
    pub fn len(&self) -> usize {
        self.expirations.len()
    }

    /// Returns true if there are no expirations.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.expirations.is_empty()
    }

    /// Gets or creates an expiration order book.
    ///
    /// If a validation config has been set via [`set_validation`](Self::set_validation),
    /// newly created expiration books will have that config propagated to their chain.
    pub fn get_or_create(&self, expiration: ExpirationDate) -> Arc<ExpirationOrderBook> {
        if let Some(entry) = self.expirations.get(&expiration) {
            return Arc::clone(entry.value());
        }
        let book = if let Some(ref reg) = self.registry {
            Arc::new(ExpirationOrderBook::new_with_registry(
                &self.underlying,
                expiration,
                Arc::clone(reg),
            ))
        } else {
            Arc::new(ExpirationOrderBook::new(&self.underlying, expiration))
        };
        if let Some(ref config) = self.validation_config.get() {
            book.set_validation(config.clone());
        }
        if let Some(ref specs) = self.contract_specs.get() {
            book.chain().set_specs(specs.clone());
        }
        let stp = self.stp_mode.get();
        if stp != STPMode::None {
            book.set_stp_mode(stp);
        }
        self.expirations.insert(expiration, Arc::clone(&book));
        book
    }

    /// Gets an expiration order book.
    ///
    /// # Errors
    ///
    /// Returns `Error::ExpirationNotFound` if the expiration does not exist.
    pub fn get(&self, expiration: &ExpirationDate) -> Result<Arc<ExpirationOrderBook>> {
        self.expirations
            .get(expiration)
            .map(|e| Arc::clone(e.value()))
            .ok_or_else(|| Error::expiration_not_found(expiration.to_string()))
    }

    /// Returns true if an expiration exists.
    #[must_use]
    pub fn contains(&self, expiration: &ExpirationDate) -> bool {
        self.expirations.contains_key(expiration)
    }

    /// Returns an iterator over all expirations.
    pub fn iter(
        &self,
    ) -> impl Iterator<Item = crossbeam_skiplist::map::Entry<'_, ExpirationDate, Arc<ExpirationOrderBook>>>
    {
        self.expirations.iter()
    }

    /// Removes an expiration order book.
    pub fn remove(&self, expiration: &ExpirationDate) -> bool {
        self.expirations.remove(expiration).is_some()
    }

    /// Returns the total order count across all expirations.
    #[must_use]
    pub fn total_order_count(&self) -> usize {
        self.expirations
            .iter()
            .map(|e| e.value().total_order_count())
            .sum()
    }

    /// Returns the total strike count across all expirations.
    #[must_use]
    pub fn total_strike_count(&self) -> usize {
        self.expirations
            .iter()
            .map(|e| e.value().strike_count())
            .sum()
    }

    /// Returns statistics about this expiration manager.
    #[must_use]
    pub fn stats(&self) -> ExpirationManagerStats {
        ExpirationManagerStats {
            underlying: self.underlying.clone(),
            expiration_count: self.len(),
            total_strikes: self.total_strike_count(),
            total_orders: self.total_order_count(),
        }
    }
}

/// Statistics about an expiration manager.
#[derive(Debug, Clone)]
pub struct ExpirationManagerStats {
    /// The underlying asset symbol.
    pub underlying: String,
    /// Number of expirations.
    pub expiration_count: usize,
    /// Total number of strikes across all expirations.
    pub total_strikes: usize,
    /// Total number of orders across all expirations.
    pub total_orders: usize,
}

impl std::fmt::Display for ExpirationManagerStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {} expirations, {} strikes, {} orders",
            self.underlying, self.expiration_count, self.total_strikes, self.total_orders
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
    fn test_expiration_order_book_creation() {
        let exp = ExpirationOrderBook::new("BTC", test_expiration());

        assert_eq!(exp.underlying(), "BTC");
        assert!(exp.is_empty());
    }

    #[test]
    fn test_expiration_order_book_strikes() {
        let exp = ExpirationOrderBook::new("BTC", test_expiration());

        drop(exp.get_or_create_strike(50000));
        drop(exp.get_or_create_strike(55000));
        drop(exp.get_or_create_strike(45000));

        assert_eq!(exp.strike_count(), 3);
        assert_eq!(exp.strike_prices(), vec![45000, 50000, 55000]);
    }

    #[test]
    fn test_expiration_order_book_orders() {
        let exp = ExpirationOrderBook::new("BTC", test_expiration());

        let strike = exp.get_or_create_strike(50000);
        strike
            .call()
            .add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();

        assert_eq!(exp.total_order_count(), 1);
    }

    #[test]
    fn test_expiration_manager_creation() {
        let manager = ExpirationOrderBookManager::new("BTC");

        assert!(manager.is_empty());
        assert_eq!(manager.underlying(), "BTC");
    }

    #[test]
    fn test_expiration_manager_get_or_create() {
        let manager = ExpirationOrderBookManager::new("BTC");

        drop(manager.get_or_create(ExpirationDate::Days(pos_or_panic!(30.0))));
        drop(manager.get_or_create(ExpirationDate::Days(pos_or_panic!(60.0))));
        drop(manager.get_or_create(ExpirationDate::Days(pos_or_panic!(90.0))));

        assert_eq!(manager.len(), 3);
    }

    #[test]
    fn test_expiration_order_book_expiration() {
        let exp = test_expiration();
        let book = ExpirationOrderBook::new("BTC", exp);
        assert_eq!(*book.expiration(), exp);
    }

    #[test]
    fn test_expiration_order_book_chain() {
        let book = ExpirationOrderBook::new("BTC", test_expiration());
        drop(book.get_or_create_strike(50000));
        let chain = book.chain();
        assert_eq!(chain.strike_count(), 1);
    }

    #[test]
    fn test_expiration_order_book_get_strike() {
        let book = ExpirationOrderBook::new("BTC", test_expiration());
        drop(book.get_or_create_strike(50000));

        assert!(book.get_strike(50000).is_ok());
        assert!(book.get_strike(99999).is_err());
    }

    #[test]
    fn test_expiration_order_book_atm_strike() {
        let book = ExpirationOrderBook::new("BTC", test_expiration());

        drop(book.get_or_create_strike(45000));
        drop(book.get_or_create_strike(50000));
        drop(book.get_or_create_strike(55000));

        assert_eq!(book.atm_strike(48000).unwrap(), 50000);
        assert_eq!(book.atm_strike(53000).unwrap(), 55000);
    }

    #[test]
    fn test_expiration_order_book_atm_strike_empty() {
        let book = ExpirationOrderBook::new("BTC", test_expiration());
        assert!(book.atm_strike(50000).is_err());
    }

    #[test]
    fn test_expiration_manager_get() {
        let manager = ExpirationOrderBookManager::new("BTC");
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
    fn test_expiration_manager_contains() {
        let manager = ExpirationOrderBookManager::new("BTC");
        let exp = test_expiration();

        drop(manager.get_or_create(exp));

        assert!(manager.contains(&exp));
        assert!(!manager.contains(&ExpirationDate::Days(pos_or_panic!(999.0))));
    }

    #[test]
    fn test_expiration_manager_remove() {
        let manager = ExpirationOrderBookManager::new("BTC");
        let exp = test_expiration();

        drop(manager.get_or_create(exp));
        assert_eq!(manager.len(), 1);

        assert!(manager.remove(&exp));
        assert_eq!(manager.len(), 0);
        assert!(!manager.remove(&exp));
    }

    #[test]
    fn test_expiration_manager_total_order_count() {
        let manager = ExpirationOrderBookManager::new("BTC");

        let exp_book = manager.get_or_create(test_expiration());
        let strike = exp_book.get_or_create_strike(50000);
        strike
            .call()
            .add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();
        drop(strike);
        drop(exp_book);

        assert_eq!(manager.total_order_count(), 1);
    }

    #[test]
    fn test_expiration_manager_total_strike_count() {
        let manager = ExpirationOrderBookManager::new("BTC");

        let exp_book = manager.get_or_create(test_expiration());
        exp_book.get_or_create_strike(50000);
        exp_book.get_or_create_strike(55000);
        drop(exp_book);

        assert_eq!(manager.total_strike_count(), 2);
    }

    #[test]
    fn test_expiration_manager_stats() {
        let manager = ExpirationOrderBookManager::new("BTC");

        let exp_book = manager.get_or_create(test_expiration());
        let strike = exp_book.get_or_create_strike(50000);
        strike
            .call()
            .add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();
        drop(strike);
        drop(exp_book);

        let stats = manager.stats();
        assert_eq!(stats.underlying, "BTC");
        assert_eq!(stats.expiration_count, 1);
        assert_eq!(stats.total_strikes, 1);
        assert_eq!(stats.total_orders, 1);

        let display = format!("{}", stats);
        assert!(display.contains("BTC"));
    }

    #[test]
    fn test_expiration_set_validation() {
        let book = ExpirationOrderBook::new("BTC", test_expiration());
        let config = ValidationConfig::new().with_tick_size(100);
        book.set_validation(config.clone());

        assert_eq!(book.validation_config(), Some(config));

        let strike = book.get_or_create_strike(50000);
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
    fn test_expiration_manager_set_validation_propagates() {
        let manager = ExpirationOrderBookManager::new("BTC");
        let config = ValidationConfig::new().with_tick_size(100);
        manager.set_validation(config);

        let exp = manager.get_or_create(test_expiration());
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
    fn test_expiration_manager_existing_book_unaffected() {
        let manager = ExpirationOrderBookManager::new("BTC");

        let exp_before = manager.get_or_create(ExpirationDate::Days(pos_or_panic!(30.0)));

        manager.set_validation(ValidationConfig::new().with_tick_size(100));

        // Existing expiration is NOT affected
        let strike = exp_before.get_or_create_strike(50000);
        assert!(
            strike
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 150, 10)
                .is_ok()
        );

        // New expiration IS affected
        let exp_after = manager.get_or_create(ExpirationDate::Days(pos_or_panic!(60.0)));
        let strike2 = exp_after.get_or_create_strike(50000);
        assert!(
            strike2
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 150, 10)
                .is_err()
        );
    }
}
