//! Option chain order book module.
//!
//! This module provides the [`OptionChainOrderBook`] and [`OptionChainOrderBookManager`]
//! for managing all strikes within a single expiration.

use super::contract_specs::{ContractSpecs, SharedContractSpecs};
use super::fees::SharedFeeSchedule;
use super::instrument_registry::InstrumentRegistry;
use super::stp::SharedSTPMode;
use super::strike::{StrikeOrderBook, StrikeOrderBookManager};
use super::validation::{SharedValidationConfig, ValidationConfig};
use crate::error::{Error, Result};
use crossbeam_skiplist::SkipMap;
use optionstratlib::ExpirationDate;
use orderbook_rs::{FeeSchedule, OrderId, STPMode};
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
    /// Instrument registry propagated to strike managers.
    /// Stored to keep the `Arc` reference alive for the hierarchy.
    #[allow(dead_code)]
    registry: Option<Arc<InstrumentRegistry>>,
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
            registry: None,
        }
    }

    /// Creates a new option chain order book with an instrument registry.
    ///
    /// The registry is propagated to the internal [`StrikeOrderBookManager`]
    /// so that newly created strikes get unique instrument IDs.
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
            strikes: Arc::new(StrikeOrderBookManager::new_with_registry(
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

    /// Sets the contract specifications for this chain.
    ///
    /// Also propagates the specs to the strike manager for newly created strikes.
    pub fn set_specs(&self, specs: ContractSpecs) {
        self.strikes.set_specs(specs);
    }

    /// Returns the current contract specifications, if any.
    ///
    /// Delegates to the strike manager to maintain a single source of truth.
    #[must_use]
    pub fn specs(&self) -> Option<ContractSpecs> {
        self.strikes.specs()
    }

    /// Sets the validation config for all future strikes created within this chain.
    ///
    /// Delegates to the underlying [`StrikeOrderBookManager::set_validation`].
    /// Existing strikes are not affected.
    pub fn set_validation(&self, config: ValidationConfig) {
        self.strikes.set_validation(config);
    }

    /// Returns the current validation config, if any.
    #[must_use]
    pub fn validation_config(&self) -> Option<ValidationConfig> {
        self.strikes.validation_config()
    }

    /// Sets the STP mode for all future option books created within this chain.
    ///
    /// Delegates to the underlying [`StrikeOrderBookManager::set_stp_mode`].
    /// Existing books are not affected.
    pub fn set_stp_mode(&self, mode: STPMode) {
        self.strikes.set_stp_mode(mode);
    }

    /// Returns the current STP mode.
    #[must_use]
    pub fn stp_mode(&self) -> STPMode {
        self.strikes.stp_mode()
    }

    /// Sets the fee schedule for all future option books created within this chain.
    ///
    /// Delegates to the underlying [`StrikeOrderBookManager::set_fee_schedule`].
    /// Existing books are not affected.
    pub fn set_fee_schedule(&self, schedule: FeeSchedule) {
        self.strikes.set_fee_schedule(schedule);
    }

    /// Returns the current fee schedule, or `None` if no fees are configured.
    #[must_use]
    pub fn fee_schedule(&self) -> Option<FeeSchedule> {
        self.strikes.fee_schedule()
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
    /// Validation config applied to newly created chains.
    validation_config: SharedValidationConfig,
    /// Contract specs propagated to newly created chains.
    contract_specs: SharedContractSpecs,
    /// Instrument registry propagated to newly created chains.
    registry: Option<Arc<InstrumentRegistry>>,
    /// STP mode propagated to newly created chains.
    stp_mode: SharedSTPMode,
    /// Fee schedule propagated to newly created chains.
    fee_schedule: SharedFeeSchedule,
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
            validation_config: SharedValidationConfig::new(),
            contract_specs: SharedContractSpecs::new(),
            registry: None,
            stp_mode: SharedSTPMode::new(),
            fee_schedule: SharedFeeSchedule::new(),
        }
    }

    /// Creates a new option chain manager with an instrument registry.
    ///
    /// The registry is propagated to newly created chains and their
    /// strike managers.
    ///
    /// # Arguments
    ///
    /// * `underlying` - The underlying asset symbol
    /// * `registry` - The instrument registry for ID allocation
    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn new_with_registry(
        underlying: impl Into<String>,
        registry: Arc<InstrumentRegistry>,
    ) -> Self {
        Self {
            chains: SkipMap::new(),
            underlying: underlying.into(),
            validation_config: SharedValidationConfig::new(),
            contract_specs: SharedContractSpecs::new(),
            registry: Some(registry),
            stp_mode: SharedSTPMode::new(),
            fee_schedule: SharedFeeSchedule::new(),
        }
    }

    /// Sets the contract specs for all future chains created by this manager.
    ///
    /// Existing chains are not affected. Only newly created chains
    /// via [`get_or_create`](Self::get_or_create) will have these specs propagated.
    pub fn set_specs(&self, specs: ContractSpecs) {
        self.contract_specs.set(specs);
    }

    /// Returns the current contract specs, if any.
    #[must_use]
    pub fn specs(&self) -> Option<ContractSpecs> {
        self.contract_specs.get()
    }

    /// Sets the validation config for all future chains created by this manager.
    ///
    /// Existing chains are not affected. Only newly created chains
    /// via [`get_or_create`](Self::get_or_create) will have this config applied.
    pub fn set_validation(&self, config: ValidationConfig) {
        self.validation_config.set(config);
    }

    /// Returns the current validation config, if any.
    #[must_use]
    pub fn validation_config(&self) -> Option<ValidationConfig> {
        self.validation_config.get()
    }

    /// Sets the STP mode for all future chains created by this manager.
    ///
    /// Existing chains are not affected. Only newly created chains
    /// via [`get_or_create`](Self::get_or_create) will have this mode propagated.
    pub fn set_stp_mode(&self, mode: STPMode) {
        self.stp_mode.set(mode);
    }

    /// Returns the current STP mode.
    #[must_use]
    pub fn stp_mode(&self) -> STPMode {
        self.stp_mode.get()
    }

    /// Sets the fee schedule for all future chains created by this manager.
    ///
    /// Existing chains are not affected. Only newly created chains
    /// via [`get_or_create`](Self::get_or_create) will have this schedule propagated.
    pub fn set_fee_schedule(&self, schedule: FeeSchedule) {
        self.fee_schedule.set(Some(schedule));
    }

    /// Returns the current fee schedule, or `None` if no fees are configured.
    #[must_use]
    pub fn fee_schedule(&self) -> Option<FeeSchedule> {
        self.fee_schedule.get()
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
    ///
    /// If a validation config has been set via [`set_validation`](Self::set_validation),
    /// newly created chains will have that config propagated to their strike manager.
    pub fn get_or_create(&self, expiration: ExpirationDate) -> Arc<OptionChainOrderBook> {
        if let Some(entry) = self.chains.get(&expiration) {
            return Arc::clone(entry.value());
        }
        let chain = if let Some(ref reg) = self.registry {
            Arc::new(OptionChainOrderBook::new_with_registry(
                &self.underlying,
                expiration,
                Arc::clone(reg),
            ))
        } else {
            Arc::new(OptionChainOrderBook::new(&self.underlying, expiration))
        };
        if let Some(ref config) = self.validation_config.get() {
            chain.set_validation(config.clone());
        }
        if let Some(ref specs) = self.contract_specs.get() {
            chain.set_specs(specs.clone());
        }
        let stp = self.stp_mode.get();
        if stp != STPMode::None {
            chain.set_stp_mode(stp);
        }
        if let Some(schedule) = self.fee_schedule.get() {
            chain.set_fee_schedule(schedule);
        }
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

    #[test]
    fn test_option_chain_set_validation() {
        let chain = OptionChainOrderBook::new("BTC", test_expiration());
        let config = ValidationConfig::new().with_tick_size(100);
        chain.set_validation(config.clone());

        assert_eq!(chain.validation_config(), Some(config));

        // New strike inherits validation
        let strike = chain.get_or_create_strike(50000);
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
    fn test_option_chain_no_validation_by_default() {
        let chain = OptionChainOrderBook::new("BTC", test_expiration());
        assert!(chain.validation_config().is_none());
    }

    #[test]
    fn test_option_chain_manager_set_validation_propagates() {
        let manager = OptionChainOrderBookManager::new("BTC");
        let config = ValidationConfig::new().with_tick_size(100);
        manager.set_validation(config);

        // New chain should inherit validation
        let chain = manager.get_or_create(test_expiration());
        let strike = chain.get_or_create_strike(50000);
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
    fn test_option_chain_manager_existing_chain_unaffected() {
        let manager = OptionChainOrderBookManager::new("BTC");

        // Create chain before setting validation
        let chain_before = manager.get_or_create(ExpirationDate::Days(pos_or_panic!(30.0)));

        // Set validation after
        manager.set_validation(ValidationConfig::new().with_tick_size(100));

        // Existing chain's new strikes are NOT affected
        let strike = chain_before.get_or_create_strike(50000);
        assert!(
            strike
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 150, 10)
                .is_ok()
        );

        // New chain IS affected
        let chain_after = manager.get_or_create(ExpirationDate::Days(pos_or_panic!(60.0)));
        let strike2 = chain_after.get_or_create_strike(50000);
        assert!(
            strike2
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 150, 10)
                .is_err()
        );
    }
}
