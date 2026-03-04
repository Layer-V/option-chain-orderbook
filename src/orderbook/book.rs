//! Option order book wrapper.
//!
//! This module provides the [`OptionOrderBook`] structure that wraps the
//! OrderBook-rs `OrderBook<T>` implementation with option-specific functionality.

use super::instrument_status::InstrumentStatus;
use super::quote::Quote;
use super::validation::ValidationConfig;
use crate::Result;
use optionstratlib::OptionStyle;
use orderbook_rs::{DefaultOrderBook, OrderBookSnapshot, OrderId, STPMode, Side, TimeInForce};
use pricelevel::Hash32;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};

/// Order book for a single option contract.
///
/// Wraps the high-performance `OrderBook<T>` from OrderBook-rs and provides
/// option-specific functionality. The underlying OrderBook uses `u64` for
/// prices (representing price in smallest units, e.g., cents or satoshis).
///
/// ## Architecture
///
/// This struct sits at the bottom of the option chain hierarchy:
/// ```text
/// UnderlyingOrderBookManager
///   └── UnderlyingOrderBook
///         └── ExpirationOrderBookManager
///               └── ExpirationOrderBook
///                     └── OptionChainOrderBook
///                           └── StrikeOrderBook
///                                 └── OptionOrderBook ← This struct
///                                       └── OrderBook<T> (from OrderBook-rs)
/// ```
pub struct OptionOrderBook {
    /// The option contract symbol.
    symbol: String,
    /// Hash of the symbol for efficient comparison.
    symbol_hash: u64,
    /// The underlying order book from OrderBook-rs.
    book: Arc<DefaultOrderBook>,
    /// Last known quote for change detection.
    last_quote: Arc<Quote>,
    /// The option style (Call or Put).
    option_style: OptionStyle,
    /// Unique identifier for this order book.
    id: OrderId,
    /// Lifecycle status of this instrument, stored as atomic u8.
    status: AtomicU8,
    /// Numeric instrument ID for fast lookups and compact wire representation.
    /// Stored as `AtomicU32` so it can be assigned after construction
    /// without requiring `&mut self`.
    instrument_id: AtomicU32,
}

impl OptionOrderBook {
    /// Creates a new option order book for the given symbol.
    ///
    /// # Arguments
    ///
    /// * `symbol` - The option contract symbol (e.g., "BTC-20240329-50000-C")
    /// * `option_style` - The option style (Call or Put)
    #[must_use]
    pub fn new(symbol: impl Into<String>, option_style: OptionStyle) -> Self {
        let symbol = symbol.into();
        let symbol_hash = Self::hash_symbol(&symbol);

        Self {
            symbol: symbol.clone(),
            symbol_hash,
            book: Arc::new(DefaultOrderBook::new(&symbol)),
            last_quote: Arc::new(Quote::empty(0)),
            option_style,
            id: OrderId::new(),
            status: AtomicU8::new(InstrumentStatus::Active as u8),
            instrument_id: AtomicU32::new(0),
        }
    }

    /// Creates a new option order book with a pre-assigned instrument ID.
    ///
    /// Used internally by the hierarchy when an [`InstrumentRegistry`](super::instrument_registry::InstrumentRegistry)
    /// is available.
    ///
    /// # Arguments
    ///
    /// * `symbol` - The option contract symbol
    /// * `option_style` - The option style (Call or Put)
    /// * `instrument_id` - The unique numeric instrument ID
    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn new_with_id(
        symbol: impl Into<String>,
        option_style: OptionStyle,
        instrument_id: u32,
    ) -> Self {
        let symbol = symbol.into();
        let symbol_hash = Self::hash_symbol(&symbol);

        Self {
            symbol: symbol.clone(),
            symbol_hash,
            book: Arc::new(DefaultOrderBook::new(&symbol)),
            last_quote: Arc::new(Quote::empty(0)),
            option_style,
            id: OrderId::new(),
            status: AtomicU8::new(InstrumentStatus::Active as u8),
            instrument_id: AtomicU32::new(instrument_id),
        }
    }

    /// Creates a new option order book with pre-trade validation configured.
    ///
    /// Validation rules are applied to the underlying `OrderBook` before it is
    /// wrapped in `Arc`, so they cannot be changed after construction.
    ///
    /// # Arguments
    ///
    /// * `symbol` - The option contract symbol (e.g., "BTC-20240329-50000-C")
    /// * `option_style` - The option style (Call or Put)
    /// * `config` - Validation configuration (tick size, lot size, min/max order size)
    #[must_use]
    pub fn new_with_validation(
        symbol: impl Into<String>,
        option_style: OptionStyle,
        config: &ValidationConfig,
    ) -> Self {
        let symbol = symbol.into();
        let symbol_hash = Self::hash_symbol(&symbol);
        let mut book = DefaultOrderBook::new(&symbol);
        Self::apply_validation(&mut book, config);

        Self {
            symbol,
            symbol_hash,
            book: Arc::new(book),
            last_quote: Arc::new(Quote::empty(0)),
            option_style,
            id: OrderId::new(),
            status: AtomicU8::new(InstrumentStatus::Active as u8),
            instrument_id: AtomicU32::new(0),
        }
    }

    /// Creates a new option order book with a pre-assigned instrument ID and
    /// pre-trade validation configured.
    ///
    /// Used internally by the hierarchy when both an
    /// [`InstrumentRegistry`](super::instrument_registry::InstrumentRegistry)
    /// and a [`ValidationConfig`] are available.
    ///
    /// # Arguments
    ///
    /// * `symbol` - The option contract symbol
    /// * `option_style` - The option style (Call or Put)
    /// * `instrument_id` - The unique numeric instrument ID
    /// * `config` - Validation configuration
    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn new_with_id_and_validation(
        symbol: impl Into<String>,
        option_style: OptionStyle,
        instrument_id: u32,
        config: &ValidationConfig,
    ) -> Self {
        let symbol = symbol.into();
        let symbol_hash = Self::hash_symbol(&symbol);
        let mut book = DefaultOrderBook::new(&symbol);
        Self::apply_validation(&mut book, config);

        Self {
            symbol,
            symbol_hash,
            book: Arc::new(book),
            last_quote: Arc::new(Quote::empty(0)),
            option_style,
            id: OrderId::new(),
            status: AtomicU8::new(InstrumentStatus::Active as u8),
            instrument_id: AtomicU32::new(instrument_id),
        }
    }

    /// Creates a new option order book with STP mode configured.
    ///
    /// # Arguments
    ///
    /// * `symbol` - The option contract symbol
    /// * `option_style` - The option style (Call or Put)
    /// * `stp_mode` - Self-trade prevention mode
    #[must_use]
    pub(crate) fn new_with_stp(
        symbol: impl Into<String>,
        option_style: OptionStyle,
        stp_mode: STPMode,
    ) -> Self {
        let symbol = symbol.into();
        let symbol_hash = Self::hash_symbol(&symbol);

        Self {
            symbol: symbol.clone(),
            symbol_hash,
            book: Arc::new(DefaultOrderBook::with_stp_mode(&symbol, stp_mode)),
            last_quote: Arc::new(Quote::empty(0)),
            option_style,
            id: OrderId::new(),
            status: AtomicU8::new(InstrumentStatus::Active as u8),
            instrument_id: AtomicU32::new(0),
        }
    }

    /// Creates a new option order book with instrument ID and STP mode.
    ///
    /// # Arguments
    ///
    /// * `symbol` - The option contract symbol
    /// * `option_style` - The option style (Call or Put)
    /// * `instrument_id` - The unique numeric instrument ID
    /// * `stp_mode` - Self-trade prevention mode
    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn new_with_id_and_stp(
        symbol: impl Into<String>,
        option_style: OptionStyle,
        instrument_id: u32,
        stp_mode: STPMode,
    ) -> Self {
        let symbol = symbol.into();
        let symbol_hash = Self::hash_symbol(&symbol);

        Self {
            symbol: symbol.clone(),
            symbol_hash,
            book: Arc::new(DefaultOrderBook::with_stp_mode(&symbol, stp_mode)),
            last_quote: Arc::new(Quote::empty(0)),
            option_style,
            id: OrderId::new(),
            status: AtomicU8::new(InstrumentStatus::Active as u8),
            instrument_id: AtomicU32::new(instrument_id),
        }
    }

    /// Creates a new option order book with validation and STP mode.
    ///
    /// # Arguments
    ///
    /// * `symbol` - The option contract symbol
    /// * `option_style` - The option style (Call or Put)
    /// * `config` - Validation configuration
    /// * `stp_mode` - Self-trade prevention mode
    #[must_use]
    pub(crate) fn new_with_validation_and_stp(
        symbol: impl Into<String>,
        option_style: OptionStyle,
        config: &ValidationConfig,
        stp_mode: STPMode,
    ) -> Self {
        let symbol = symbol.into();
        let symbol_hash = Self::hash_symbol(&symbol);
        let mut book = DefaultOrderBook::with_stp_mode(&symbol, stp_mode);
        Self::apply_validation(&mut book, config);

        Self {
            symbol,
            symbol_hash,
            book: Arc::new(book),
            last_quote: Arc::new(Quote::empty(0)),
            option_style,
            id: OrderId::new(),
            status: AtomicU8::new(InstrumentStatus::Active as u8),
            instrument_id: AtomicU32::new(0),
        }
    }

    /// Creates a new option order book with instrument ID, validation, and STP mode.
    ///
    /// # Arguments
    ///
    /// * `symbol` - The option contract symbol
    /// * `option_style` - The option style (Call or Put)
    /// * `instrument_id` - The unique numeric instrument ID
    /// * `config` - Validation configuration
    /// * `stp_mode` - Self-trade prevention mode
    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn new_with_id_validation_and_stp(
        symbol: impl Into<String>,
        option_style: OptionStyle,
        instrument_id: u32,
        config: &ValidationConfig,
        stp_mode: STPMode,
    ) -> Self {
        let symbol = symbol.into();
        let symbol_hash = Self::hash_symbol(&symbol);
        let mut book = DefaultOrderBook::with_stp_mode(&symbol, stp_mode);
        Self::apply_validation(&mut book, config);

        Self {
            symbol,
            symbol_hash,
            book: Arc::new(book),
            last_quote: Arc::new(Quote::empty(0)),
            option_style,
            id: OrderId::new(),
            status: AtomicU8::new(InstrumentStatus::Active as u8),
            instrument_id: AtomicU32::new(instrument_id),
        }
    }

    /// Applies validation config to a mutable order book before wrapping in `Arc`.
    fn apply_validation(book: &mut DefaultOrderBook, config: &ValidationConfig) {
        if let Some(tick) = config.tick_size() {
            book.set_tick_size(tick);
        }
        if let Some(lot) = config.lot_size() {
            book.set_lot_size(lot);
        }
        if let Some(min) = config.min_order_size() {
            book.set_min_order_size(min);
        }
        if let Some(max) = config.max_order_size() {
            book.set_max_order_size(max);
        }
    }

    /// Returns the current validation configuration read back from the underlying book,
    /// or `None` if no validation rules are configured.
    #[must_use]
    pub fn validation_config(&self) -> Option<ValidationConfig> {
        let mut config = ValidationConfig::new();
        if let Some(tick) = self.book.tick_size() {
            config = config.with_tick_size(tick);
        }
        if let Some(lot) = self.book.lot_size() {
            config = config.with_lot_size(lot);
        }
        if let Some(min) = self.book.min_order_size() {
            config = config.with_min_order_size(min);
        }
        if let Some(max) = self.book.max_order_size() {
            config = config.with_max_order_size(max);
        }
        if config.is_empty() {
            None
        } else {
            Some(config)
        }
    }

    /// Returns the option style (Call or Put).
    #[must_use]
    pub const fn option_style(&self) -> OptionStyle {
        self.option_style
    }

    /// Computes a hash for the symbol.
    fn hash_symbol(symbol: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        symbol.hash(&mut hasher);
        hasher.finish()
    }

    /// Returns the option contract symbol.
    #[must_use]
    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    /// Returns the symbol hash.
    #[must_use]
    pub const fn symbol_hash(&self) -> u64 {
        self.symbol_hash
    }

    /// Returns the unique identifier for this order book.
    #[must_use]
    pub const fn id(&self) -> OrderId {
        self.id
    }

    /// Returns a reference to the underlying OrderBook from OrderBook-rs.
    #[must_use]
    pub fn inner(&self) -> &DefaultOrderBook {
        &self.book
    }

    /// Returns an Arc reference to the underlying OrderBook.
    #[must_use]
    pub fn inner_arc(&self) -> Arc<DefaultOrderBook> {
        Arc::clone(&self.book)
    }

    /// Returns the numeric instrument ID.
    ///
    /// Returns 0 for standalone books created outside the hierarchy.
    /// Hierarchy-created books get unique IDs from the
    /// [`InstrumentRegistry`](super::instrument_registry::InstrumentRegistry).
    #[must_use]
    #[inline]
    pub fn instrument_id(&self) -> u32 {
        self.instrument_id.load(Ordering::Relaxed)
    }

    /// Sets the instrument ID after construction.
    ///
    /// Used by the hierarchy to assign IDs only after confirming the book
    /// won the insertion race in [`StrikeOrderBookManager::get_or_create`](super::strike::StrikeOrderBookManager::get_or_create).
    #[inline]
    pub(crate) fn set_instrument_id(&self, id: u32) {
        self.instrument_id.store(id, Ordering::Relaxed);
    }

    /// Returns the configured self-trade prevention mode.
    ///
    /// [`STPMode::None`] means STP is disabled (default).
    #[must_use]
    #[inline]
    pub fn stp_mode(&self) -> STPMode {
        self.book.stp_mode()
    }

    /// Returns the current lifecycle status of this instrument.
    #[must_use]
    #[inline]
    pub fn status(&self) -> InstrumentStatus {
        let raw = self.status.load(Ordering::Acquire);
        // SAFETY: we only ever store valid InstrumentStatus u8 values.
        // Fail closed: corrupted values reject orders instead of accepting them.
        InstrumentStatus::from_u8(raw).unwrap_or(InstrumentStatus::Halted)
    }

    /// Sets the lifecycle status of this instrument.
    ///
    /// # Arguments
    ///
    /// * `status` - The new status to set
    #[inline]
    pub fn set_status(&self, status: InstrumentStatus) {
        self.status.store(status as u8, Ordering::Release);
    }

    /// Halts the instrument, preventing new orders from being accepted.
    ///
    /// Existing resting orders are not cancelled. Use [`expire`](Self::expire)
    /// to both halt and cancel all orders.
    #[inline]
    pub fn halt(&self) {
        self.set_status(InstrumentStatus::Halted);
    }

    /// Resumes the instrument, allowing new orders to be accepted.
    #[inline]
    pub fn resume(&self) {
        self.set_status(InstrumentStatus::Active);
    }

    /// Expires the instrument, cancelling all resting orders.
    ///
    /// Sets status to [`Expired`](InstrumentStatus::Expired), collects all
    /// resting order IDs, and clears the book.
    ///
    /// # Returns
    ///
    /// A vector of order IDs that were cancelled.
    pub fn expire(&self) -> Vec<OrderId> {
        self.set_status(InstrumentStatus::Expired);
        let orders = self.book.get_all_orders();
        let ids: Vec<OrderId> = orders.iter().map(|o| o.id()).collect();
        self.clear();
        ids
    }

    /// Checks that the instrument is accepting orders, returning an error if not.
    fn check_active(&self) -> Result<()> {
        let current = self.status();
        if current.is_accepting_orders() {
            Ok(())
        } else {
            Err(crate::Error::instrument_not_active(
                &self.symbol,
                current.to_string(),
            ))
        }
    }

    /// Adds a limit order to the book.
    ///
    /// # Arguments
    ///
    /// * `order_id` - Unique identifier for the order
    /// * `side` - Buy or Sell side
    /// * `price` - Limit price in smallest units (u128)
    /// * `quantity` - Order quantity in smallest units (u64)
    ///
    /// # Errors
    ///
    /// Returns [`InstrumentNotActive`](crate::Error::InstrumentNotActive) if the instrument is not
    /// [`Active`](InstrumentStatus::Active).
    pub fn add_limit_order(
        &self,
        order_id: OrderId,
        side: Side,
        price: u128,
        quantity: u64,
    ) -> Result<()> {
        self.check_active()?;
        self.book
            .add_limit_order(order_id, price, quantity, side, TimeInForce::Gtc, None)
            .map_err(|e| crate::Error::orderbook(e.to_string()))?;
        Ok(())
    }

    /// Adds a limit order with time-in-force specification.
    ///
    /// # Arguments
    ///
    /// * `order_id` - Unique identifier for the order
    /// * `side` - Buy or Sell side
    /// * `price` - Limit price in smallest units (u128)
    /// * `quantity` - Order quantity in smallest units (u64)
    /// * `tif` - Time-in-force (GTC, IOC, FOK, etc.)
    ///
    /// # Errors
    ///
    /// Returns [`InstrumentNotActive`](crate::Error::InstrumentNotActive) if the instrument is not
    /// [`Active`](InstrumentStatus::Active).
    pub fn add_limit_order_with_tif(
        &self,
        order_id: OrderId,
        side: Side,
        price: u128,
        quantity: u64,
        tif: TimeInForce,
    ) -> Result<()> {
        self.check_active()?;
        self.book
            .add_limit_order(order_id, price, quantity, side, tif, None)
            .map_err(|e| crate::Error::orderbook(e.to_string()))?;
        Ok(())
    }

    /// Adds a limit order with user identity for self-trade prevention.
    ///
    /// When STP is enabled on this book, the `user_id` is used to detect
    /// self-trades. Use [`Hash32::zero()`] to bypass STP checks.
    ///
    /// # Arguments
    ///
    /// * `order_id` - Unique identifier for the order
    /// * `side` - Buy or Sell side
    /// * `price` - Limit price in smallest units (u128)
    /// * `quantity` - Order quantity in smallest units (u64)
    /// * `user_id` - Owner identity for STP checks
    ///
    /// # Errors
    ///
    /// - [`InstrumentNotActive`](crate::Error::InstrumentNotActive) if the instrument is not
    ///   [`Active`](InstrumentStatus::Active).
    /// - [`OrderBookError`](crate::Error::OrderBookError) if the upstream book rejects the order
    ///   (e.g., `MissingUserId` when STP is enabled and `user_id` is zero).
    pub fn add_limit_order_with_user(
        &self,
        order_id: OrderId,
        side: Side,
        price: u128,
        quantity: u64,
        user_id: Hash32,
    ) -> Result<()> {
        self.check_active()?;
        self.book
            .add_limit_order_with_user(
                order_id,
                price,
                quantity,
                side,
                TimeInForce::Gtc,
                user_id,
                None,
            )
            .map_err(|e| crate::Error::orderbook(e.to_string()))?;
        Ok(())
    }

    /// Adds a limit order with time-in-force and user identity for STP.
    ///
    /// Combines time-in-force specification with self-trade prevention.
    ///
    /// # Arguments
    ///
    /// * `order_id` - Unique identifier for the order
    /// * `side` - Buy or Sell side
    /// * `price` - Limit price in smallest units (u128)
    /// * `quantity` - Order quantity in smallest units (u64)
    /// * `tif` - Time-in-force (GTC, IOC, FOK, etc.)
    /// * `user_id` - Owner identity for STP checks
    ///
    /// # Errors
    ///
    /// - [`InstrumentNotActive`](crate::Error::InstrumentNotActive) if the instrument is not
    ///   [`Active`](InstrumentStatus::Active).
    /// - [`OrderBookError`](crate::Error::OrderBookError) if the upstream book rejects the order.
    pub fn add_limit_order_with_tif_and_user(
        &self,
        order_id: OrderId,
        side: Side,
        price: u128,
        quantity: u64,
        tif: TimeInForce,
        user_id: Hash32,
    ) -> Result<()> {
        self.check_active()?;
        self.book
            .add_limit_order_with_user(order_id, price, quantity, side, tif, user_id, None)
            .map_err(|e| crate::Error::orderbook(e.to_string()))?;
        Ok(())
    }

    /// Cancels an order by its ID.
    ///
    /// # Arguments
    ///
    /// * `order_id` - The ID of the order to cancel
    ///
    /// # Returns
    ///
    /// `Ok(true)` if the order was found and cancelled, `Ok(false)` if not found.
    pub fn cancel_order(&self, order_id: OrderId) -> Result<bool> {
        match self.book.cancel_order(order_id) {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    /// Returns the current best quote.
    #[must_use]
    pub fn best_quote(&self) -> Quote {
        let timestamp_ms = orderbook_rs::current_time_millis();

        let (bid_price, bid_size) = self
            .book
            .best_bid()
            .map(|p| (Some(p), self.bid_depth_at_price(p)))
            .unwrap_or((None, 0));

        let (ask_price, ask_size) = self
            .book
            .best_ask()
            .map(|p| (Some(p), self.ask_depth_at_price(p)))
            .unwrap_or((None, 0));

        Quote::new(bid_price, bid_size, ask_price, ask_size, timestamp_ms)
    }

    /// Returns the best bid price.
    #[must_use]
    pub fn best_bid(&self) -> Option<u128> {
        self.book.best_bid()
    }

    /// Returns the best ask price.
    #[must_use]
    pub fn best_ask(&self) -> Option<u128> {
        self.book.best_ask()
    }

    /// Returns the mid price if both sides exist.
    #[must_use]
    pub fn mid_price(&self) -> Option<f64> {
        self.book.mid_price()
    }

    /// Returns the spread if both sides exist.
    #[must_use]
    pub fn spread(&self) -> Option<u128> {
        self.book.spread()
    }

    /// Returns the spread in basis points.
    #[must_use]
    pub fn spread_bps(&self) -> Option<f64> {
        self.book.spread_bps(None)
    }

    /// Returns a snapshot of the order book.
    ///
    /// # Arguments
    ///
    /// * `depth` - Maximum number of price levels to include on each side
    #[must_use]
    pub fn snapshot(&self, depth: usize) -> OrderBookSnapshot {
        self.book.create_snapshot(depth)
    }

    /// Returns the total bid depth (sum of all bid quantities).
    #[must_use]
    pub fn total_bid_depth(&self) -> u64 {
        self.book.total_depth_at_levels(usize::MAX, Side::Buy)
    }

    /// Returns the total ask depth (sum of all ask quantities).
    #[must_use]
    pub fn total_ask_depth(&self) -> u64 {
        self.book.total_depth_at_levels(usize::MAX, Side::Sell)
    }

    /// Returns the number of bid price levels.
    #[must_use]
    pub fn bid_level_count(&self) -> usize {
        self.book.get_bids().len()
    }

    /// Returns the number of ask price levels.
    #[must_use]
    pub fn ask_level_count(&self) -> usize {
        self.book.get_asks().len()
    }

    /// Returns the total number of orders in the book.
    #[must_use]
    pub fn order_count(&self) -> usize {
        self.book.get_all_orders().len()
    }

    /// Returns true if the order book is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.book.best_bid().is_none() && self.book.best_ask().is_none()
    }

    /// Clears all orders from the book.
    pub fn clear(&self) {
        let empty_snapshot = OrderBookSnapshot {
            symbol: self.symbol.clone(),
            timestamp: orderbook_rs::current_time_millis(),
            bids: vec![],
            asks: vec![],
        };
        let _ = self.book.restore_from_snapshot(empty_snapshot);
    }

    /// Returns the order book imbalance for top N levels.
    ///
    /// Calculated as `(bid_depth - ask_depth) / (bid_depth + ask_depth)`.
    /// Returns a value between -1.0 (all asks) and 1.0 (all bids).
    ///
    /// # Arguments
    ///
    /// * `levels` - Number of price levels to consider
    #[must_use]
    pub fn imbalance(&self, levels: usize) -> f64 {
        self.book.order_book_imbalance(levels)
    }

    /// Updates the last known quote and returns true if it changed.
    pub fn update_last_quote(&mut self) -> bool {
        let current = self.best_quote();
        let changed = current != *self.last_quote;
        self.last_quote = Arc::new(current);
        changed
    }

    /// Returns a reference to the last known quote.
    #[must_use]
    pub fn last_quote(&self) -> &Quote {
        &self.last_quote
    }

    /// Returns an Arc reference to the last known quote.
    #[must_use]
    pub fn last_quote_arc(&self) -> Arc<Quote> {
        Arc::clone(&self.last_quote)
    }

    /// Returns depth at a specific price level on the bid side.
    #[must_use]
    pub fn bid_depth_at_price(&self, price: u128) -> u64 {
        let (bid_volumes, _) = self.book.get_volume_by_price();
        bid_volumes.get(&price).copied().unwrap_or(0)
    }

    /// Returns depth at a specific price level on the ask side.
    #[must_use]
    pub fn ask_depth_at_price(&self, price: u128) -> u64 {
        let (_, ask_volumes) = self.book.get_volume_by_price();
        ask_volumes.get(&price).copied().unwrap_or(0)
    }

    /// Calculates VWAP for a given quantity.
    ///
    /// # Arguments
    ///
    /// * `quantity` - Target quantity to fill
    /// * `side` - Side to calculate VWAP for
    #[must_use]
    pub fn vwap(&self, quantity: u64, side: Side) -> Option<f64> {
        self.book.vwap(quantity, side)
    }

    /// Returns the micro price (weighted by volume at best bid/ask).
    #[must_use]
    pub fn micro_price(&self) -> Option<f64> {
        self.book.micro_price()
    }

    /// Calculates market impact for a hypothetical order.
    #[must_use]
    pub fn market_impact(&self, quantity: u64, side: Side) -> orderbook_rs::MarketImpact {
        self.book.market_impact(quantity, side)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_option_order_book_creation() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);

        assert_eq!(book.symbol(), "BTC-20240329-50000-C");
        assert_eq!(book.option_style(), OptionStyle::Call);
        assert!(book.is_empty());
        assert_eq!(book.order_count(), 0);
    }

    #[test]
    fn test_add_limit_orders() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);

        book.add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();
        book.add_limit_order(OrderId::new(), Side::Sell, 101, 5)
            .unwrap();

        assert_eq!(book.order_count(), 2);
        assert_eq!(book.bid_level_count(), 1);
        assert_eq!(book.ask_level_count(), 1);
    }

    #[test]
    fn test_best_quote() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);

        book.add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();
        book.add_limit_order(OrderId::new(), Side::Sell, 101, 5)
            .unwrap();

        let quote = book.best_quote();

        assert_eq!(quote.bid_price(), Some(100));
        assert_eq!(quote.bid_size(), 10);
        assert_eq!(quote.ask_price(), Some(101));
        assert_eq!(quote.ask_size(), 5);
        assert!(quote.is_two_sided());
    }

    #[test]
    fn test_mid_price_and_spread() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);

        book.add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();
        book.add_limit_order(OrderId::new(), Side::Sell, 102, 5)
            .unwrap();

        assert_eq!(book.mid_price(), Some(101.0));
        assert_eq!(book.spread(), Some(2));
    }

    #[test]
    fn test_cancel_order() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);

        let order_id = OrderId::new();
        book.add_limit_order(order_id, Side::Buy, 100, 10).unwrap();
        assert_eq!(book.order_count(), 1);

        let cancelled = book.cancel_order(order_id).unwrap();
        assert!(cancelled);
        assert_eq!(book.order_count(), 0);
    }

    #[test]
    fn test_total_depth() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);

        book.add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();
        book.add_limit_order(OrderId::new(), Side::Buy, 99, 20)
            .unwrap();
        book.add_limit_order(OrderId::new(), Side::Sell, 101, 5)
            .unwrap();

        assert_eq!(book.total_bid_depth(), 30);
        assert_eq!(book.total_ask_depth(), 5);
    }

    #[test]
    fn test_symbol_hash() {
        let book1 = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);
        let book2 = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);
        let book3 = OptionOrderBook::new("BTC-20240329-50000-P", OptionStyle::Put);

        assert_eq!(book1.symbol_hash(), book2.symbol_hash());
        assert_ne!(book1.symbol_hash(), book3.symbol_hash());
    }

    #[test]
    fn test_imbalance() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);

        book.add_limit_order(OrderId::new(), Side::Buy, 100, 60)
            .unwrap();
        book.add_limit_order(OrderId::new(), Side::Sell, 101, 40)
            .unwrap();

        // Imbalance = (60 - 40) / (60 + 40) = 0.2
        let imbalance = book.imbalance(5);
        assert!((imbalance - 0.2).abs() < 0.01);
    }

    #[test]
    fn test_inner_access() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);
        let _inner = book.inner();
        assert!(book.is_empty());
    }

    #[test]
    fn test_inner_arc_access() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);
        let _inner_arc = book.inner_arc();
        assert!(book.is_empty());
    }

    #[test]
    fn test_add_limit_order_with_tif() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);

        book.add_limit_order_with_tif(OrderId::new(), Side::Buy, 100, 10, TimeInForce::Gtc)
            .unwrap();

        assert_eq!(book.order_count(), 1);
    }

    #[test]
    fn test_best_bid_ask() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);

        assert!(book.best_bid().is_none());
        assert!(book.best_ask().is_none());

        book.add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();
        book.add_limit_order(OrderId::new(), Side::Sell, 105, 5)
            .unwrap();

        assert_eq!(book.best_bid(), Some(100));
        assert_eq!(book.best_ask(), Some(105));
    }

    #[test]
    fn test_spread_bps() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);

        book.add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();
        book.add_limit_order(OrderId::new(), Side::Sell, 102, 5)
            .unwrap();

        let spread_bps = book.spread_bps();
        assert!(spread_bps.is_some());
    }

    #[test]
    fn test_snapshot() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);

        book.add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();
        book.add_limit_order(OrderId::new(), Side::Sell, 105, 5)
            .unwrap();

        let snapshot = book.snapshot(5);
        assert_eq!(snapshot.bids.len(), 1);
        assert_eq!(snapshot.asks.len(), 1);
    }

    #[test]
    fn test_clear() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);

        book.add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();
        book.add_limit_order(OrderId::new(), Side::Sell, 105, 5)
            .unwrap();

        assert_eq!(book.order_count(), 2);
        book.clear();
        assert!(book.is_empty());
    }

    #[test]
    fn test_update_last_quote() {
        let mut book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);

        book.add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();

        let changed = book.update_last_quote();
        assert!(changed);

        let changed_again = book.update_last_quote();
        assert!(!changed_again);

        let _last = book.last_quote();
    }

    #[test]
    fn test_depth_at_price() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);

        book.add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();
        book.add_limit_order(OrderId::new(), Side::Sell, 105, 5)
            .unwrap();

        assert_eq!(book.bid_depth_at_price(100), 10);
        assert_eq!(book.bid_depth_at_price(99), 0);
        assert_eq!(book.ask_depth_at_price(105), 5);
        assert_eq!(book.ask_depth_at_price(106), 0);
    }

    #[test]
    fn test_vwap() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);

        book.add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();
        book.add_limit_order(OrderId::new(), Side::Buy, 99, 10)
            .unwrap();
        book.add_limit_order(OrderId::new(), Side::Sell, 105, 10)
            .unwrap();

        let vwap_sell = book.vwap(5, Side::Sell);
        assert!(vwap_sell.is_some());

        let vwap_buy = book.vwap(5, Side::Buy);
        assert!(vwap_buy.is_some());
    }

    #[test]
    fn test_micro_price() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);

        book.add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();
        book.add_limit_order(OrderId::new(), Side::Sell, 102, 10)
            .unwrap();

        let micro = book.micro_price();
        assert!(micro.is_some());
    }

    #[test]
    fn test_market_impact() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);

        book.add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();
        book.add_limit_order(OrderId::new(), Side::Sell, 105, 10)
            .unwrap();

        let impact = book.market_impact(5, Side::Buy);
        // avg_price is f64, just verify it's a valid number
        assert!(impact.avg_price >= 0.0 || impact.avg_price < 0.0);
    }

    #[test]
    fn test_new_with_validation_tick_size() {
        let config = ValidationConfig::new().with_tick_size(100);
        let book = OptionOrderBook::new_with_validation(
            "BTC-20240329-50000-C",
            OptionStyle::Call,
            &config,
        );

        // Valid price (multiple of 100)
        assert!(
            book.add_limit_order(OrderId::new(), Side::Buy, 200, 10)
                .is_ok()
        );

        // Invalid price (not a multiple of 100)
        assert!(
            book.add_limit_order(OrderId::new(), Side::Buy, 150, 10)
                .is_err()
        );
    }

    #[test]
    fn test_new_with_validation_lot_size() {
        let config = ValidationConfig::new().with_lot_size(10);
        let book = OptionOrderBook::new_with_validation(
            "BTC-20240329-50000-C",
            OptionStyle::Call,
            &config,
        );

        // Valid quantity (multiple of 10)
        assert!(
            book.add_limit_order(OrderId::new(), Side::Buy, 100, 20)
                .is_ok()
        );

        // Invalid quantity (not a multiple of 10)
        assert!(
            book.add_limit_order(OrderId::new(), Side::Buy, 100, 15)
                .is_err()
        );
    }

    #[test]
    fn test_new_with_validation_min_max_order_size() {
        let config = ValidationConfig::new()
            .with_min_order_size(5)
            .with_max_order_size(100);
        let book = OptionOrderBook::new_with_validation(
            "BTC-20240329-50000-C",
            OptionStyle::Call,
            &config,
        );

        // Valid quantity (within range)
        assert!(
            book.add_limit_order(OrderId::new(), Side::Buy, 100, 50)
                .is_ok()
        );

        // Too small
        assert!(
            book.add_limit_order(OrderId::new(), Side::Buy, 100, 2)
                .is_err()
        );

        // Too large
        assert!(
            book.add_limit_order(OrderId::new(), Side::Buy, 100, 200)
                .is_err()
        );
    }

    #[test]
    fn test_validation_config_readback() {
        let config = ValidationConfig::new()
            .with_tick_size(100)
            .with_lot_size(10)
            .with_min_order_size(1)
            .with_max_order_size(1000);
        let book = OptionOrderBook::new_with_validation(
            "BTC-20240329-50000-C",
            OptionStyle::Call,
            &config,
        );

        let readback = book.validation_config();
        assert_eq!(readback, Some(config));
    }

    #[test]
    fn test_no_validation_by_default() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);
        assert!(book.validation_config().is_none());

        // Any price/quantity should work
        assert!(
            book.add_limit_order(OrderId::new(), Side::Buy, 1, 1)
                .is_ok()
        );
        assert!(
            book.add_limit_order(OrderId::new(), Side::Buy, 150, 7)
                .is_ok()
        );
    }

    #[test]
    fn test_new_with_empty_validation() {
        let config = ValidationConfig::new();
        let book = OptionOrderBook::new_with_validation(
            "BTC-20240329-50000-C",
            OptionStyle::Call,
            &config,
        );

        // Empty config = no validation = anything goes
        assert!(
            book.add_limit_order(OrderId::new(), Side::Buy, 1, 1)
                .is_ok()
        );
    }

    // ── Instrument status tests ──────────────────────────────────────────

    #[test]
    fn test_default_status_is_active() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);
        assert_eq!(book.status(), InstrumentStatus::Active);
    }

    #[test]
    fn test_default_status_is_active_with_validation() {
        let config = ValidationConfig::new().with_tick_size(100);
        let book = OptionOrderBook::new_with_validation(
            "BTC-20240329-50000-C",
            OptionStyle::Call,
            &config,
        );
        assert_eq!(book.status(), InstrumentStatus::Active);
    }

    #[test]
    fn test_set_status_and_get() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);

        for &status in &[
            InstrumentStatus::Pending,
            InstrumentStatus::Active,
            InstrumentStatus::Halted,
            InstrumentStatus::Settling,
            InstrumentStatus::Expired,
        ] {
            book.set_status(status);
            assert_eq!(book.status(), status);
        }
    }

    #[test]
    fn test_halt_sets_halted() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);
        assert_eq!(book.status(), InstrumentStatus::Active);

        book.halt();
        assert_eq!(book.status(), InstrumentStatus::Halted);
    }

    #[test]
    fn test_resume_sets_active() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);
        book.halt();
        assert_eq!(book.status(), InstrumentStatus::Halted);

        book.resume();
        assert_eq!(book.status(), InstrumentStatus::Active);
    }

    #[test]
    fn test_expire_sets_expired_and_clears() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);

        let id1 = OrderId::new();
        let id2 = OrderId::new();
        book.add_limit_order(id1, Side::Buy, 100, 10).unwrap();
        book.add_limit_order(id2, Side::Sell, 105, 5).unwrap();
        assert_eq!(book.order_count(), 2);

        let cancelled = book.expire();
        assert_eq!(book.status(), InstrumentStatus::Expired);
        assert!(book.is_empty());
        assert_eq!(cancelled.len(), 2);
        assert!(cancelled.contains(&id1));
        assert!(cancelled.contains(&id2));
    }

    #[test]
    fn test_expire_on_empty_book() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);
        let cancelled = book.expire();
        assert_eq!(book.status(), InstrumentStatus::Expired);
        assert!(cancelled.is_empty());
    }

    #[test]
    fn test_order_rejected_when_halted() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);
        book.halt();

        let result = book.add_limit_order(OrderId::new(), Side::Buy, 100, 10);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("instrument not active"));
        assert!(err.to_string().contains("Halted"));
    }

    #[test]
    fn test_order_rejected_when_pending() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);
        book.set_status(InstrumentStatus::Pending);

        let result = book.add_limit_order(OrderId::new(), Side::Buy, 100, 10);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Pending"));
    }

    #[test]
    fn test_order_rejected_when_settling() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);
        book.set_status(InstrumentStatus::Settling);

        let result = book.add_limit_order(OrderId::new(), Side::Buy, 100, 10);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Settling"));
    }

    #[test]
    fn test_order_rejected_when_expired() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);
        book.set_status(InstrumentStatus::Expired);

        let result = book.add_limit_order(OrderId::new(), Side::Buy, 100, 10);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Expired"));
    }

    #[test]
    fn test_order_rejected_with_tif_when_not_active() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);
        book.halt();

        let result =
            book.add_limit_order_with_tif(OrderId::new(), Side::Buy, 100, 10, TimeInForce::Gtc);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Halted"));
    }

    #[test]
    fn test_orders_accepted_after_resume() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);

        book.halt();
        assert!(
            book.add_limit_order(OrderId::new(), Side::Buy, 100, 10)
                .is_err()
        );

        book.resume();
        assert!(
            book.add_limit_order(OrderId::new(), Side::Buy, 100, 10)
                .is_ok()
        );
    }

    #[test]
    fn test_halt_preserves_existing_orders() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);

        book.add_limit_order(OrderId::new(), Side::Buy, 100, 10)
            .unwrap();
        assert_eq!(book.order_count(), 1);

        book.halt();
        // Existing orders remain
        assert_eq!(book.order_count(), 1);
        assert_eq!(book.best_bid(), Some(100));
    }

    #[test]
    fn test_cancel_works_when_halted() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);

        let oid = OrderId::new();
        book.add_limit_order(oid, Side::Buy, 100, 10).unwrap();
        book.halt();

        // Cancellation should still work on halted instruments
        let cancelled = book.cancel_order(oid).unwrap();
        assert!(cancelled);
        assert!(book.is_empty());
    }

    #[test]
    fn test_default_instrument_id_is_zero() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);
        assert_eq!(book.instrument_id(), 0);
    }

    #[test]
    fn test_new_with_id() {
        let book = OptionOrderBook::new_with_id("BTC-20240329-50000-C", OptionStyle::Call, 42);
        assert_eq!(book.instrument_id(), 42);
        assert_eq!(book.symbol(), "BTC-20240329-50000-C");
    }

    #[test]
    fn test_new_with_validation_has_zero_id() {
        let config = super::super::validation::ValidationConfig::new().with_tick_size(10);
        let book = OptionOrderBook::new_with_validation(
            "BTC-20240329-50000-C",
            OptionStyle::Call,
            &config,
        );
        assert_eq!(book.instrument_id(), 0);
    }

    #[test]
    fn test_new_with_id_and_validation() {
        let config = super::super::validation::ValidationConfig::new().with_tick_size(10);
        let book = OptionOrderBook::new_with_id_and_validation(
            "BTC-20240329-50000-C",
            OptionStyle::Call,
            99,
            &config,
        );
        assert_eq!(book.instrument_id(), 99);
        // Verify validation is applied
        let vc = book.validation_config();
        assert!(vc.is_some());
        assert_eq!(vc.unwrap().tick_size(), Some(10));
    }

    #[test]
    fn test_stp_mode_default_is_none() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);
        assert_eq!(book.stp_mode(), STPMode::None);
    }

    #[test]
    fn test_new_with_stp_cancel_taker() {
        let book = OptionOrderBook::new_with_stp(
            "BTC-20240329-50000-C",
            OptionStyle::Call,
            STPMode::CancelTaker,
        );
        assert_eq!(book.stp_mode(), STPMode::CancelTaker);
        assert_eq!(book.instrument_id(), 0);
    }

    #[test]
    fn test_new_with_id_and_stp() {
        let book = OptionOrderBook::new_with_id_and_stp(
            "BTC-20240329-50000-C",
            OptionStyle::Call,
            42,
            STPMode::CancelMaker,
        );
        assert_eq!(book.stp_mode(), STPMode::CancelMaker);
        assert_eq!(book.instrument_id(), 42);
    }

    #[test]
    fn test_new_with_validation_and_stp() {
        let config = super::super::validation::ValidationConfig::new().with_tick_size(10);
        let book = OptionOrderBook::new_with_validation_and_stp(
            "BTC-20240329-50000-C",
            OptionStyle::Call,
            &config,
            STPMode::CancelBoth,
        );
        assert_eq!(book.stp_mode(), STPMode::CancelBoth);
        assert_eq!(
            book.validation_config().map(|c| c.tick_size()),
            Some(Some(10))
        );
    }

    #[test]
    fn test_new_with_id_validation_and_stp() {
        let config = super::super::validation::ValidationConfig::new().with_tick_size(10);
        let book = OptionOrderBook::new_with_id_validation_and_stp(
            "BTC-20240329-50000-C",
            OptionStyle::Call,
            7,
            &config,
            STPMode::CancelTaker,
        );
        assert_eq!(book.stp_mode(), STPMode::CancelTaker);
        assert_eq!(book.instrument_id(), 7);
        assert_eq!(
            book.validation_config().map(|c| c.tick_size()),
            Some(Some(10))
        );
    }

    #[test]
    fn test_add_limit_order_with_user() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);
        let user = Hash32::from([1u8; 32]);
        let result = book.add_limit_order_with_user(OrderId::new(), Side::Buy, 100, 10, user);
        assert!(result.is_ok());
        assert_eq!(book.order_count(), 1);
    }

    #[test]
    fn test_add_limit_order_with_tif_and_user() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);
        let user = Hash32::from([2u8; 32]);
        let result = book.add_limit_order_with_tif_and_user(
            OrderId::new(),
            Side::Sell,
            200,
            5,
            TimeInForce::Gtc,
            user,
        );
        assert!(result.is_ok());
        assert_eq!(book.order_count(), 1);
    }

    #[test]
    fn test_stp_cancel_taker_prevents_self_trade() {
        let book = OptionOrderBook::new_with_stp(
            "BTC-20240329-50000-C",
            OptionStyle::Call,
            STPMode::CancelTaker,
        );
        let user = Hash32::from([1u8; 32]);

        // Place a resting sell order
        book.add_limit_order_with_user(OrderId::new(), Side::Sell, 100, 10, user)
            .unwrap();
        assert_eq!(book.order_count(), 1);

        // Same user places a crossing buy — STP triggers, returns error
        let result = book.add_limit_order_with_user(OrderId::new(), Side::Buy, 100, 10, user);
        assert!(result.is_err());
        // Maker (sell) should still be there
        assert_eq!(book.order_count(), 1);
        assert!(book.best_ask().is_some());
    }

    #[test]
    fn test_stp_cancel_maker_removes_resting_order() {
        let book = OptionOrderBook::new_with_stp(
            "BTC-20240329-50000-C",
            OptionStyle::Call,
            STPMode::CancelMaker,
        );
        let user = Hash32::from([1u8; 32]);

        // Place a resting sell order
        book.add_limit_order_with_user(OrderId::new(), Side::Sell, 100, 10, user)
            .unwrap();
        assert_eq!(book.order_count(), 1);

        // Same user places a crossing buy — maker cancelled, taker rests
        book.add_limit_order_with_user(OrderId::new(), Side::Buy, 100, 10, user)
            .unwrap();
        // Taker (buy) should now be resting, maker (sell) was cancelled
        assert_eq!(book.order_count(), 1);
        assert!(book.best_bid().is_some());
    }

    #[test]
    fn test_stp_cancel_both_removes_all() {
        let book = OptionOrderBook::new_with_stp(
            "BTC-20240329-50000-C",
            OptionStyle::Call,
            STPMode::CancelBoth,
        );
        let user = Hash32::from([1u8; 32]);

        // Place a resting sell order
        book.add_limit_order_with_user(OrderId::new(), Side::Sell, 100, 10, user)
            .unwrap();
        assert_eq!(book.order_count(), 1);

        // Same user places a crossing buy — STP triggers, returns error
        let result = book.add_limit_order_with_user(OrderId::new(), Side::Buy, 100, 10, user);
        assert!(result.is_err());
    }

    #[test]
    fn test_stp_different_users_trade_normally() {
        let book = OptionOrderBook::new_with_stp(
            "BTC-20240329-50000-C",
            OptionStyle::Call,
            STPMode::CancelTaker,
        );
        let user_a = Hash32::from([1u8; 32]);
        let user_b = Hash32::from([2u8; 32]);

        // User A sells
        book.add_limit_order_with_user(OrderId::new(), Side::Sell, 100, 10, user_a)
            .unwrap();

        // User B buys — should trade normally
        book.add_limit_order_with_user(OrderId::new(), Side::Buy, 100, 10, user_b)
            .unwrap();
        // Both matched and removed
        assert_eq!(book.order_count(), 0);
    }

    #[test]
    fn test_add_limit_order_with_user_rejected_when_halted() {
        let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);
        book.halt();
        let user = Hash32::from([1u8; 32]);
        let result = book.add_limit_order_with_user(OrderId::new(), Side::Buy, 100, 10, user);
        assert!(result.is_err());
    }
}
