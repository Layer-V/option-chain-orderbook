//! Strike order book module.
//!
//! This module provides the [`StrikeOrderBook`] and [`StrikeOrderBookManager`]
//! for managing call/put pairs at a specific strike price.

use super::book::{BookConfig, OptionOrderBook};
use super::contract_specs::{ContractSpecs, SharedContractSpecs};
use super::fees::SharedFeeSchedule;
use super::instrument_registry::{InstrumentInfo, InstrumentRegistry};
use super::quote::Quote;
use super::stp::SharedSTPMode;
use super::validation::{SharedValidationConfig, ValidationConfig};
use crate::error::{Error, Result};
use crate::utils::format_expiration_yyyymmdd;
use crossbeam_skiplist::SkipMap;
use optionstratlib::greeks::Greek;
use optionstratlib::{ExpirationDate, OptionStyle};
use orderbook_rs::{FeeSchedule, OrderId, STPMode};
use std::sync::Arc;

/// Order book for a single strike price containing both call and put.
///
/// This struct manages the call/put pair at a specific strike price.
///
/// ## Architecture
///
/// ```text
/// StrikeOrderBook (per strike price)
///   ├── OptionOrderBook (call)
///   │     └── OrderBook<T> (from OrderBook-rs)
///   └── OptionOrderBook (put)
///         └── OrderBook<T> (from OrderBook-rs)
/// ```
pub struct StrikeOrderBook {
    /// The underlying asset symbol (e.g., "BTC").
    underlying: String,
    /// The expiration date.
    expiration: ExpirationDate,
    /// The strike price.
    strike: u64,
    /// Call option order book.
    call: Arc<OptionOrderBook>,
    /// Put option order book.
    put: Arc<OptionOrderBook>,
    /// Greeks for the call option.
    call_greeks: Option<Greek>,
    /// Greeks for the put option.
    put_greeks: Option<Greek>,
    /// Unique identifier for this strike order book.
    id: OrderId,
}

impl StrikeOrderBook {
    /// Creates a new strike order book.
    ///
    /// # Arguments
    ///
    /// * `underlying` - The underlying asset symbol (e.g., "BTC")
    /// * `expiration` - The expiration date
    /// * `strike` - The strike price
    #[must_use]
    pub fn new(underlying: impl Into<String>, expiration: ExpirationDate, strike: u64) -> Self {
        let underlying = underlying.into();

        // Format expiration as YYYYMMDD, fallback to Display if formatting fails
        let exp_str =
            format_expiration_yyyymmdd(&expiration).unwrap_or_else(|_| expiration.to_string());

        let call_symbol = format!("{}-{}-{}-C", underlying, exp_str, strike);
        let put_symbol = format!("{}-{}-{}-P", underlying, exp_str, strike);

        Self {
            underlying,
            expiration,
            strike,
            call: Arc::new(OptionOrderBook::new(call_symbol, OptionStyle::Call)),
            put: Arc::new(OptionOrderBook::new(put_symbol, OptionStyle::Put)),
            call_greeks: None,
            put_greeks: None,
            id: OrderId::new(),
        }
    }

    /// Creates a new strike order book with pre-trade validation configured.
    ///
    /// The validation config is applied to both call and put order books
    /// during construction.
    ///
    /// # Arguments
    ///
    /// * `underlying` - The underlying asset symbol (e.g., "BTC")
    /// * `expiration` - The expiration date
    /// * `strike` - The strike price
    /// * `config` - Validation configuration for both call and put books
    #[must_use]
    pub fn new_with_validation(
        underlying: impl Into<String>,
        expiration: ExpirationDate,
        strike: u64,
        config: &ValidationConfig,
    ) -> Self {
        let underlying = underlying.into();

        let exp_str =
            format_expiration_yyyymmdd(&expiration).unwrap_or_else(|_| expiration.to_string());

        let call_symbol = format!("{}-{}-{}-C", underlying, exp_str, strike);
        let put_symbol = format!("{}-{}-{}-P", underlying, exp_str, strike);

        Self {
            underlying,
            expiration,
            strike,
            call: Arc::new(OptionOrderBook::new_with_validation(
                call_symbol,
                OptionStyle::Call,
                config,
            )),
            put: Arc::new(OptionOrderBook::new_with_validation(
                put_symbol,
                OptionStyle::Put,
                config,
            )),
            call_greeks: None,
            put_greeks: None,
            id: OrderId::new(),
        }
    }

    /// Creates a new strike order book from pre-built call/put order books.
    ///
    /// Used internally by [`StrikeOrderBookManager`] when an instrument registry
    /// is available, so that each `OptionOrderBook` already has a unique
    /// instrument ID assigned.
    #[must_use]
    pub(crate) fn from_books(
        underlying: impl Into<String>,
        expiration: ExpirationDate,
        strike: u64,
        call: Arc<OptionOrderBook>,
        put: Arc<OptionOrderBook>,
    ) -> Self {
        Self {
            underlying: underlying.into(),
            expiration,
            strike,
            call,
            put,
            call_greeks: None,
            put_greeks: None,
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

    /// Returns the strike price.
    #[must_use]
    pub const fn strike(&self) -> u64 {
        self.strike
    }

    /// Returns the unique identifier for this strike order book.
    #[must_use]
    pub const fn id(&self) -> OrderId {
        self.id
    }

    /// Returns the STP mode configured on the call book.
    ///
    /// Both call and put books share the same STP mode when created
    /// through the hierarchy, so reading from the call book is sufficient.
    #[must_use]
    #[inline]
    pub fn stp_mode(&self) -> STPMode {
        self.call.stp_mode()
    }

    /// Returns the fee schedule configured on the call book.
    ///
    /// Both call and put books share the same fee schedule when created
    /// through the hierarchy, so reading from the call book is sufficient.
    #[must_use]
    #[inline]
    pub fn fee_schedule(&self) -> Option<FeeSchedule> {
        self.call.fee_schedule()
    }

    /// Returns a reference to the call order book.
    #[must_use]
    pub fn call(&self) -> &OptionOrderBook {
        &self.call
    }

    /// Returns an Arc reference to the call order book.
    #[must_use]
    pub fn call_arc(&self) -> Arc<OptionOrderBook> {
        Arc::clone(&self.call)
    }

    /// Returns a reference to the put order book.
    #[must_use]
    pub fn put(&self) -> &OptionOrderBook {
        &self.put
    }

    /// Returns an Arc reference to the put order book.
    #[must_use]
    pub fn put_arc(&self) -> Arc<OptionOrderBook> {
        Arc::clone(&self.put)
    }

    /// Returns the order book for the specified option style.
    #[must_use]
    pub fn get(&self, option_style: OptionStyle) -> &OptionOrderBook {
        match option_style {
            OptionStyle::Call => &self.call,
            OptionStyle::Put => &self.put,
        }
    }

    /// Returns an Arc reference to the order book for the specified option style.
    #[must_use]
    pub fn get_arc(&self, option_style: OptionStyle) -> Arc<OptionOrderBook> {
        match option_style {
            OptionStyle::Call => Arc::clone(&self.call),
            OptionStyle::Put => Arc::clone(&self.put),
        }
    }

    /// Returns the best quote for the call option.
    #[must_use]
    pub fn call_quote(&self) -> Quote {
        self.call.best_quote()
    }

    /// Returns the best quote for the put option.
    #[must_use]
    pub fn put_quote(&self) -> Quote {
        self.put.best_quote()
    }

    /// Returns true if both call and put have two-sided quotes.
    #[must_use]
    pub fn is_fully_quoted(&self) -> bool {
        self.call.best_quote().is_two_sided() && self.put.best_quote().is_two_sided()
    }

    /// Returns the total order count across call and put.
    #[must_use]
    pub fn order_count(&self) -> usize {
        self.call.order_count() + self.put.order_count()
    }

    /// Returns true if both call and put are empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.call.is_empty() && self.put.is_empty()
    }

    /// Clears all orders from both call and put books.
    pub fn clear(&self) {
        self.call.clear();
        self.put.clear();
    }

    /// Updates the Greeks for the call option.
    pub fn update_call_greeks(&mut self, greeks: Greek) {
        self.call_greeks = Some(greeks);
    }

    /// Updates the Greeks for the put option.
    pub fn update_put_greeks(&mut self, greeks: Greek) {
        self.put_greeks = Some(greeks);
    }

    /// Returns the Greeks for the call option.
    #[must_use]
    pub const fn call_greeks(&self) -> Option<&Greek> {
        self.call_greeks.as_ref()
    }

    /// Returns the Greeks for the put option.
    #[must_use]
    pub const fn put_greeks(&self) -> Option<&Greek> {
        self.put_greeks.as_ref()
    }
}

/// Manages strike order books for a single expiration.
///
/// Provides centralized access to all strikes within an expiration.
/// Uses `SkipMap` for thread-safe concurrent access.
pub struct StrikeOrderBookManager {
    /// Strike order books indexed by strike price.
    strikes: SkipMap<u64, Arc<StrikeOrderBook>>,
    /// The underlying asset symbol.
    underlying: String,
    /// The expiration date.
    expiration: ExpirationDate,
    /// Validation config applied to newly created strike books.
    validation_config: SharedValidationConfig,
    /// Contract specs propagated to newly created strike books.
    contract_specs: SharedContractSpecs,
    /// Instrument registry for allocating IDs to new option books.
    registry: Option<Arc<InstrumentRegistry>>,
    /// STP mode applied to newly created option books.
    stp_mode: SharedSTPMode,
    /// Fee schedule applied to newly created option books.
    fee_schedule: SharedFeeSchedule,
}

impl StrikeOrderBookManager {
    /// Creates a new strike order book manager.
    ///
    /// # Arguments
    ///
    /// * `underlying` - The underlying asset symbol
    /// * `expiration` - The expiration date
    #[must_use]
    pub fn new(underlying: impl Into<String>, expiration: ExpirationDate) -> Self {
        Self {
            strikes: SkipMap::new(),
            underlying: underlying.into(),
            expiration,
            validation_config: SharedValidationConfig::new(),
            contract_specs: SharedContractSpecs::new(),
            registry: None,
            stp_mode: SharedSTPMode::new(),
            fee_schedule: SharedFeeSchedule::new(),
        }
    }

    /// Creates a new strike order book manager with an instrument registry.
    ///
    /// When the registry is present, newly created strike books will have
    /// their call/put [`OptionOrderBook`] instances assigned unique IDs.
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
        Self {
            strikes: SkipMap::new(),
            underlying: underlying.into(),
            expiration,
            validation_config: SharedValidationConfig::new(),
            contract_specs: SharedContractSpecs::new(),
            registry: Some(registry),
            stp_mode: SharedSTPMode::new(),
            fee_schedule: SharedFeeSchedule::new(),
        }
    }

    /// Returns a reference to the instrument registry, if any.
    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn registry(&self) -> Option<&Arc<InstrumentRegistry>> {
        self.registry.as_ref()
    }

    /// Sets the contract specs associated with this manager.
    ///
    /// These specs are stored on the manager and can be retrieved later
    /// via [`specs`](Self::specs). This does not modify any existing
    /// strike books or automatically apply to newly created ones.
    pub fn set_specs(&self, specs: ContractSpecs) {
        self.contract_specs.set(specs);
    }

    /// Returns the current contract specs, if any.
    #[must_use]
    pub fn specs(&self) -> Option<ContractSpecs> {
        self.contract_specs.get()
    }

    /// Sets the validation config for all future strike books created by this manager.
    ///
    /// Existing strike books are not affected. Only newly created books
    /// via [`get_or_create`](Self::get_or_create) will use this config.
    pub fn set_validation(&self, config: ValidationConfig) {
        self.validation_config.set(config);
    }

    /// Returns the current validation config, if any.
    #[must_use]
    pub fn validation_config(&self) -> Option<ValidationConfig> {
        self.validation_config.get()
    }

    /// Sets the STP mode for all future option books created by this manager.
    ///
    /// Existing books are not affected. Only newly created books
    /// via [`get_or_create`](Self::get_or_create) will use this mode.
    #[inline]
    pub fn set_stp_mode(&self, mode: STPMode) {
        self.stp_mode.set(mode);
    }

    /// Returns the current STP mode.
    #[must_use]
    #[inline]
    pub fn stp_mode(&self) -> STPMode {
        self.stp_mode.get()
    }

    /// Sets the fee schedule for all future strike books.
    ///
    /// Existing books are not affected. Only newly created books
    /// via [`get_or_create`](Self::get_or_create) will use this schedule.
    #[inline]
    pub fn set_fee_schedule(&self, schedule: FeeSchedule) {
        self.fee_schedule.set(Some(schedule));
    }

    /// Clears the fee schedule so future strike books have no fees configured.
    ///
    /// Existing books are not affected. Only newly created books
    /// via [`get_or_create`](Self::get_or_create) will be affected.
    #[inline]
    pub fn clear_fee_schedule(&self) {
        self.fee_schedule.set(None);
    }

    /// Returns the current fee schedule, or `None` if no fees are configured.
    #[must_use]
    #[inline]
    pub fn fee_schedule(&self) -> Option<FeeSchedule> {
        self.fee_schedule.get()
    }

    /// Returns the underlying asset symbol.
    #[must_use]
    pub fn underlying(&self) -> &str {
        &self.underlying
    }

    /// Returns the expiration date.
    #[must_use = "returns the expiration date without modifying the manager"]
    pub const fn expiration(&self) -> &ExpirationDate {
        &self.expiration
    }

    /// Returns the number of strikes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.strikes.len()
    }

    /// Returns true if there are no strikes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.strikes.is_empty()
    }

    /// Gets or creates a strike order book, returning an Arc reference.
    ///
    /// If a validation config has been set via [`set_validation`](Self::set_validation),
    /// newly created strike books will have that config applied.
    ///
    /// If an instrument registry is present, each new call/put
    /// [`OptionOrderBook`] is assigned a unique instrument ID and registered
    /// in the reverse index.
    ///
    /// Uses a check-insert-check pattern: if two threads race to create the
    /// same strike, only the first insertion's book survives and only that
    /// book's IDs are registered in the reverse index.
    pub fn get_or_create(&self, strike: u64) -> Arc<StrikeOrderBook> {
        if let Some(entry) = self.strikes.get(&strike) {
            return Arc::clone(entry.value());
        }

        // Build the book without allocating IDs yet.
        let book = self.create_strike_book_without_ids(strike);
        self.strikes.insert(strike, Arc::clone(&book));

        // Re-check: another thread may have inserted first.
        if let Some(entry) = self.strikes.get(&strike) {
            let winner = Arc::clone(entry.value());
            if Arc::ptr_eq(&winner, &book) {
                // We won the race — allocate and register IDs now.
                self.assign_instrument_ids(&winner, strike);
            }
            winner
        } else {
            book
        }
    }

    /// Internal helper that builds a [`StrikeOrderBook`] with optional
    /// validation config, STP mode, and fee schedule but **without** allocating
    /// instrument IDs.
    ///
    /// IDs are assigned later by [`assign_instrument_ids`](Self::assign_instrument_ids)
    /// only after confirming the book won the insertion race.
    fn create_strike_book_without_ids(&self, strike: u64) -> Arc<StrikeOrderBook> {
        let exp_str = format_expiration_yyyymmdd(&self.expiration)
            .unwrap_or_else(|_| self.expiration.to_string());
        let call_symbol = format!("{}-{}-{}-C", self.underlying, exp_str, strike);
        let put_symbol = format!("{}-{}-{}-P", self.underlying, exp_str, strike);

        let base_config = BookConfig {
            validation: self.validation_config.get(),
            stp_mode: self.stp_mode.get(),
            fee_schedule: self.fee_schedule.get(),
            ..BookConfig::default()
        };

        let call = Arc::new(OptionOrderBook::new_with_config(
            &call_symbol,
            OptionStyle::Call,
            base_config.clone(),
        ));
        let put = Arc::new(OptionOrderBook::new_with_config(
            &put_symbol,
            OptionStyle::Put,
            base_config,
        ));

        Arc::new(StrikeOrderBook::from_books(
            &self.underlying,
            self.expiration,
            strike,
            call,
            put,
        ))
    }

    /// Assigns unique instrument IDs and registers the call/put books in the
    /// reverse index. Called only after confirming this book won the insertion race.
    fn assign_instrument_ids(&self, book: &StrikeOrderBook, strike: u64) {
        if let Some(reg) = &self.registry {
            let exp_str = format_expiration_yyyymmdd(&self.expiration)
                .unwrap_or_else(|_| self.expiration.to_string());
            let call_symbol = format!("{}-{}-{}-C", self.underlying, exp_str, strike);
            let put_symbol = format!("{}-{}-{}-P", self.underlying, exp_str, strike);

            let call_id = reg.allocate();
            let put_id = reg.allocate();

            book.call().set_instrument_id(call_id);
            book.put().set_instrument_id(put_id);

            Self::register_pair(
                reg,
                &call_symbol,
                &put_symbol,
                call_id,
                put_id,
                self.expiration,
                strike,
            );
        }
    }

    /// Registers a call/put pair in the instrument registry.
    fn register_pair(
        reg: &InstrumentRegistry,
        call_symbol: &str,
        put_symbol: &str,
        call_id: u32,
        put_id: u32,
        expiration: ExpirationDate,
        strike: u64,
    ) {
        reg.register(
            call_id,
            InstrumentInfo::new(call_symbol, expiration, strike, OptionStyle::Call),
        );
        reg.register(
            put_id,
            InstrumentInfo::new(put_symbol, expiration, strike, OptionStyle::Put),
        );
    }

    /// Gets a strike order book by strike price.
    ///
    /// # Errors
    ///
    /// Returns `Error::StrikeNotFound` if the strike does not exist.
    pub fn get(&self, strike: u64) -> Result<Arc<StrikeOrderBook>> {
        self.strikes
            .get(&strike)
            .map(|e| Arc::clone(e.value()))
            .ok_or_else(|| Error::strike_not_found(strike))
    }

    /// Returns true if a strike exists.
    #[must_use]
    pub fn contains(&self, strike: u64) -> bool {
        self.strikes.contains_key(&strike)
    }

    /// Returns an iterator over all strikes.
    pub fn iter(
        &self,
    ) -> impl Iterator<Item = crossbeam_skiplist::map::Entry<'_, u64, Arc<StrikeOrderBook>>> {
        self.strikes.iter()
    }

    /// Removes a strike order book.
    ///
    /// Note: Returns true if the strike was removed, false if it didn't exist.
    pub fn remove(&self, strike: u64) -> bool {
        self.strikes.remove(&strike).is_some()
    }

    /// Returns all strike prices (sorted).
    /// SkipMap maintains sorted order, so no additional sorting needed.
    pub fn strike_prices(&self) -> Vec<u64> {
        self.strikes.iter().map(|e| *e.key()).collect()
    }

    /// Returns the total order count across all strikes.
    #[must_use]
    pub fn total_order_count(&self) -> usize {
        self.strikes.iter().map(|e| e.value().order_count()).sum()
    }

    /// Returns the ATM (at-the-money) strike closest to the given spot price.
    ///
    /// # Errors
    ///
    /// Returns `Error::NoDataAvailable` if there are no strikes.
    pub fn atm_strike(&self, spot: u64) -> Result<u64> {
        self.strikes
            .iter()
            .map(|e| *e.key())
            .min_by_key(|&k| (k as i64 - spot as i64).unsigned_abs())
            .ok_or_else(|| Error::no_data("no strikes available"))
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
    fn test_strike_order_book_creation() {
        let strike = StrikeOrderBook::new("BTC", test_expiration(), 50000);

        assert_eq!(strike.underlying(), "BTC");
        assert_eq!(strike.strike(), 50000);
        assert!(strike.is_empty());
    }

    #[test]
    fn test_strike_order_book_orders() {
        let strike = StrikeOrderBook::new("BTC", test_expiration(), 50000);

        strike
            .call()
            .add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();
        strike
            .put()
            .add_limit_order(OrderId::new(), Side::Sell, 50, 5)
            .unwrap();

        assert_eq!(strike.order_count(), 2);
        assert!(!strike.is_empty());
    }

    #[test]
    fn test_strike_manager_creation() {
        let manager = StrikeOrderBookManager::new("BTC", test_expiration());

        assert!(manager.is_empty());
        assert_eq!(manager.len(), 0);
        assert_eq!(manager.underlying(), "BTC");
    }

    #[test]
    fn test_strike_manager_get_or_create() {
        let manager = StrikeOrderBookManager::new("BTC", test_expiration());

        {
            let strike = manager.get_or_create(50000);
            assert_eq!(strike.strike(), 50000);
        }

        drop(manager.get_or_create(55000));
        drop(manager.get_or_create(45000));

        assert_eq!(manager.len(), 3);

        let strikes = manager.strike_prices();
        assert_eq!(strikes, vec![45000, 50000, 55000]);
    }

    #[test]
    fn test_strike_manager_atm() {
        let manager = StrikeOrderBookManager::new("BTC", test_expiration());

        drop(manager.get_or_create(45000));
        drop(manager.get_or_create(50000));
        drop(manager.get_or_create(55000));

        assert_eq!(manager.atm_strike(48000).unwrap(), 50000);
        assert_eq!(manager.atm_strike(52000).unwrap(), 50000);
        assert_eq!(manager.atm_strike(53000).unwrap(), 55000);
    }

    #[test]
    fn test_strike_manager_atm_empty() {
        let manager = StrikeOrderBookManager::new("BTC", test_expiration());
        assert!(manager.atm_strike(50000).is_err());
    }

    #[test]
    fn test_strike_expiration() {
        let exp = test_expiration();
        let strike = StrikeOrderBook::new("BTC", exp, 50000);
        assert_eq!(*strike.expiration(), exp);
    }

    #[test]
    fn test_strike_call_mut() {
        let strike = StrikeOrderBook::new("BTC", test_expiration(), 50000);
        let call_arc = strike.call_arc();
        call_arc
            .add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();
        assert_eq!(strike.call().order_count(), 1);
    }

    #[test]
    fn test_strike_put_mut() {
        let strike = StrikeOrderBook::new("BTC", test_expiration(), 50000);
        let put_arc = strike.put_arc();
        put_arc
            .add_limit_order(OrderId::new(), Side::Buy, 50, 10)
            .unwrap();
        assert_eq!(strike.put().order_count(), 1);
    }

    #[test]
    fn test_strike_get_by_style() {
        let strike = StrikeOrderBook::new("BTC", test_expiration(), 50000);

        strike
            .call()
            .add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();
        strike
            .put()
            .add_limit_order(OrderId::new(), Side::Buy, 50, 5)
            .unwrap();

        let call = strike.get(OptionStyle::Call);
        let put = strike.get(OptionStyle::Put);

        assert_eq!(call.order_count(), 1);
        assert_eq!(put.order_count(), 1);
    }

    #[test]
    fn test_strike_get_arc_by_style() {
        let strike = StrikeOrderBook::new("BTC", test_expiration(), 50000);

        strike
            .get_arc(OptionStyle::Call)
            .add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();
        strike
            .get_arc(OptionStyle::Put)
            .add_limit_order(OrderId::new(), Side::Buy, 50, 5)
            .unwrap();

        assert_eq!(strike.order_count(), 2);
    }

    #[test]
    fn test_strike_quotes() {
        let strike = StrikeOrderBook::new("BTC", test_expiration(), 50000);

        strike
            .call()
            .add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();
        strike
            .call()
            .add_limit_order(OrderId::new(), Side::Sell, 110, 5)
            .unwrap();
        strike
            .put()
            .add_limit_order(OrderId::new(), Side::Buy, 50, 10)
            .unwrap();
        strike
            .put()
            .add_limit_order(OrderId::new(), Side::Sell, 60, 5)
            .unwrap();

        let call_quote = strike.call_quote();
        let put_quote = strike.put_quote();

        assert!(call_quote.is_two_sided());
        assert!(put_quote.is_two_sided());
    }

    #[test]
    fn test_strike_is_fully_quoted() {
        let strike = StrikeOrderBook::new("BTC", test_expiration(), 50000);

        assert!(!strike.is_fully_quoted());

        strike
            .call()
            .add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();
        strike
            .call()
            .add_limit_order(OrderId::new(), Side::Sell, 110, 5)
            .unwrap();

        assert!(!strike.is_fully_quoted());

        strike
            .put()
            .add_limit_order(OrderId::new(), Side::Buy, 50, 10)
            .unwrap();
        strike
            .put()
            .add_limit_order(OrderId::new(), Side::Sell, 60, 5)
            .unwrap();

        assert!(strike.is_fully_quoted());
    }

    #[test]
    fn test_strike_clear() {
        let strike = StrikeOrderBook::new("BTC", test_expiration(), 50000);

        strike
            .call()
            .add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();
        strike
            .put()
            .add_limit_order(OrderId::new(), Side::Buy, 50, 5)
            .unwrap();

        assert_eq!(strike.order_count(), 2);
        strike.clear();
        assert!(strike.is_empty());
    }

    #[test]
    fn test_strike_greeks() {
        use optionstratlib::greeks::Greek;
        use rust_decimal_macros::dec;

        let mut strike = StrikeOrderBook::new("BTC", test_expiration(), 50000);

        assert!(strike.call_greeks().is_none());
        assert!(strike.put_greeks().is_none());

        let call_greeks = Greek {
            delta: dec!(0.5),
            gamma: dec!(0.01),
            theta: dec!(-0.05),
            vega: dec!(0.2),
            rho: dec!(0.1),
            rho_d: dec!(0.0),
            alpha: dec!(0.0),
            vanna: dec!(0.0),
            vomma: dec!(0.0),
            veta: dec!(0.0),
            charm: dec!(0.0),
            color: dec!(0.0),
        };
        let put_greeks = Greek {
            delta: dec!(-0.5),
            gamma: dec!(0.01),
            theta: dec!(-0.05),
            vega: dec!(0.2),
            rho: dec!(-0.1),
            rho_d: dec!(0.0),
            alpha: dec!(0.0),
            vanna: dec!(0.0),
            vomma: dec!(0.0),
            veta: dec!(0.0),
            charm: dec!(0.0),
            color: dec!(0.0),
        };

        strike.update_call_greeks(call_greeks);
        strike.update_put_greeks(put_greeks);

        assert!(strike.call_greeks().is_some());
        assert!(strike.put_greeks().is_some());
    }

    #[test]
    fn test_strike_manager_get() {
        let manager = StrikeOrderBookManager::new("BTC", test_expiration());

        drop(manager.get_or_create(50000));

        assert!(manager.get(50000).is_ok());
        assert!(manager.get(99999).is_err());
    }

    #[test]
    fn test_strike_manager_contains() {
        let manager = StrikeOrderBookManager::new("BTC", test_expiration());

        drop(manager.get_or_create(50000));

        assert!(manager.contains(50000));
        assert!(!manager.contains(99999));
    }

    #[test]
    fn test_strike_manager_remove() {
        let manager = StrikeOrderBookManager::new("BTC", test_expiration());

        drop(manager.get_or_create(50000));
        drop(manager.get_or_create(55000));

        assert_eq!(manager.len(), 2);
        assert!(manager.remove(50000));
        assert_eq!(manager.len(), 1);
        assert!(!manager.remove(50000));
    }

    #[test]
    fn test_strike_manager_total_order_count() {
        let manager = StrikeOrderBookManager::new("BTC", test_expiration());

        let strike = manager.get_or_create(50000);
        strike
            .call()
            .add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();
        drop(strike);

        let strike2 = manager.get_or_create(55000);
        strike2
            .call()
            .add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();
        drop(strike2);

        assert_eq!(manager.total_order_count(), 2);
    }

    #[test]
    fn test_strike_with_validation_propagates_to_call_and_put() {
        let config = ValidationConfig::new().with_tick_size(100);
        let strike = StrikeOrderBook::new_with_validation("BTC", test_expiration(), 50000, &config);

        // Call: valid tick
        assert!(
            strike
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 200, 10)
                .is_ok()
        );
        // Call: invalid tick
        assert!(
            strike
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 150, 10)
                .is_err()
        );
        // Put: valid tick
        assert!(
            strike
                .put()
                .add_limit_order(OrderId::new(), Side::Buy, 300, 10)
                .is_ok()
        );
        // Put: invalid tick
        assert!(
            strike
                .put()
                .add_limit_order(OrderId::new(), Side::Buy, 250, 10)
                .is_err()
        );
    }

    #[test]
    fn test_strike_manager_set_validation_propagates() {
        let manager = StrikeOrderBookManager::new("BTC", test_expiration());

        let config = ValidationConfig::new().with_tick_size(100);
        manager.set_validation(config.clone());

        assert_eq!(manager.validation_config(), Some(config));

        // New strike should inherit validation
        let strike = manager.get_or_create(50000);
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
    fn test_strike_manager_no_validation_by_default() {
        let manager = StrikeOrderBookManager::new("BTC", test_expiration());

        assert!(manager.validation_config().is_none());

        let strike = manager.get_or_create(50000);
        // No validation: any price works
        assert!(
            strike
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 150, 10)
                .is_ok()
        );
    }

    #[test]
    fn test_strike_manager_existing_strike_unaffected() {
        let manager = StrikeOrderBookManager::new("BTC", test_expiration());

        // Create strike before setting validation
        let strike_before = manager.get_or_create(50000);

        // Now set validation
        manager.set_validation(ValidationConfig::new().with_tick_size(100));

        // Existing strike is NOT affected (any price still works)
        assert!(
            strike_before
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 150, 10)
                .is_ok()
        );

        // New strike IS affected
        let strike_after = manager.get_or_create(55000);
        assert!(
            strike_after
                .call()
                .add_limit_order(OrderId::new(), Side::Buy, 150, 10)
                .is_err()
        );
    }
}
