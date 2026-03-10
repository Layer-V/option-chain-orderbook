//! Sequencer integration for deterministic ordering and replay.
//!
//! This module provides sequencer support for the option chain hierarchy,
//! enabling deterministic ordering of all operations and journal-based replay
//! for disaster recovery and state verification.
//!
//! # Architecture
//!
//! Each [`SequencedUnderlyingOrderBook`] owns its own sequencer, providing:
//!
//! - Monotonic sequence numbers per underlying
//! - Independent journaling and replay per underlying
//! - Parallel operation across different underlyings
//! - Isolated failure domains
//!
//! # Feature Gate
//!
//! This module is only available when the `sequencer` feature is enabled:
//!
//! ```toml
//! [dependencies]
//! option-chain-orderbook = { version = "0.4", features = ["sequencer"] }
//! ```

use crate::error::Error;
use crate::orderbook::book::TerminalOrderSummary;
use crate::orderbook::underlying::UnderlyingOrderBook;
use optionstratlib::ExpirationDate;
use orderbook_rs::{OrderId, Side};
use pricelevel::Hash32;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Scope for mass cancel operations in the option chain hierarchy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MassCancelScope {
    /// Cancel across the entire underlying (all expirations, all strikes).
    Underlying,
    /// Cancel within a specific expiration.
    Expiration(ExpirationDate),
    /// Cancel within a specific strike of an expiration.
    Strike {
        /// The expiration date.
        expiration: ExpirationDate,
        /// The strike price.
        strike: u64,
    },
    /// Cancel within a specific option book (call or put).
    Book(String),
}

/// Type of mass cancel operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MassCancelType {
    /// Cancel all orders.
    All,
    /// Cancel orders on a specific side.
    BySide(Side),
    /// Cancel orders belonging to a specific user.
    ByUser(Hash32),
}

/// Command for the option chain sequencer with hierarchy routing.
///
/// Each variant represents an operation that can be sequenced through
/// the option chain sequencer. Commands are serializable for journal
/// persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OptionChainCommand {
    /// Add a limit order to a specific option book.
    AddOrder {
        /// Target option symbol (e.g., "BTC-20240329-50000-C").
        symbol: String,
        /// Order identifier.
        order_id: OrderId,
        /// Buy or Sell.
        side: Side,
        /// Limit price.
        price: u128,
        /// Order quantity.
        quantity: u64,
    },
    /// Cancel an order in a specific option book.
    CancelOrder {
        /// Target option symbol.
        symbol: String,
        /// Order to cancel.
        order_id: OrderId,
    },
    /// Mass cancel across a hierarchy level.
    MassCancel {
        /// Scope of the mass cancel operation.
        scope: MassCancelScope,
        /// Type of cancellation.
        cancel_type: MassCancelType,
    },
}

/// Result of executing an option chain command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OptionChainResult {
    /// An order was successfully added.
    OrderAdded {
        /// The identifier of the newly added order.
        order_id: OrderId,
    },
    /// An order was successfully cancelled.
    OrderCancelled {
        /// The identifier of the cancelled order.
        order_id: OrderId,
    },
    /// A mass cancel operation was executed.
    MassCancelled {
        /// Number of cancelled orders.
        cancelled_count: usize,
    },
    /// The command was rejected.
    Rejected {
        /// Human-readable reason for the rejection.
        reason: String,
    },
    /// The target book was not found.
    BookNotFound {
        /// The symbol that was not found.
        symbol: String,
    },
}

impl OptionChainResult {
    /// Returns true if this result represents a rejection or error.
    #[must_use]
    #[inline]
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Rejected { .. } | Self::BookNotFound { .. })
    }

    /// Returns true if this result represents a successful operation.
    #[must_use]
    #[inline]
    pub fn is_success(&self) -> bool {
        !self.is_error()
    }
}

/// A sequenced event emitted after processing an option chain command.
///
/// Every event carries a monotonically increasing `sequence_num` and a
/// nanosecond-precision `timestamp_ns`, enabling deterministic replay
/// and total ordering of all option chain operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionChainEvent {
    /// Monotonically increasing sequence number.
    pub sequence_num: u64,
    /// Wall-clock timestamp in nanoseconds since Unix epoch.
    pub timestamp_ns: u64,
    /// The command that was executed.
    pub command: OptionChainCommand,
    /// The result of executing the command.
    pub result: OptionChainResult,
}

/// Receipt returned after submitting a command to the sequencer.
#[derive(Debug, Clone)]
pub struct OptionChainReceipt {
    /// The sequence number assigned to this command.
    pub sequence_num: u64,
    /// The timestamp when the command was processed.
    pub timestamp_ns: u64,
    /// The result of the command.
    pub result: OptionChainResult,
}

/// Journal trait for option chain events.
///
/// Provides append and read operations for [`OptionChainEvent`] persistence,
/// replacing the upstream `Journal<()>` placeholder encoding with a
/// purpose-built abstraction.
///
/// # Thread Safety
///
/// Implementations must be `Send + Sync`. The intended pattern is
/// single-writer (the sequencer) with concurrent readers (replay,
/// monitoring).
pub trait OptionChainJournal: Send + Sync {
    /// Appends an event to the journal.
    ///
    /// The event must be durably persisted before this method returns.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] if serialization or I/O fails.
    fn append(&self, event: &OptionChainEvent) -> Result<(), Error>;

    /// Reads events starting from the given sequence number (inclusive).
    ///
    /// Returns events in sequence order. If `sequence` is beyond the
    /// last entry, returns an empty `Vec`.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] if deserialization or I/O fails.
    fn read_from(&self, sequence: u64) -> Result<Vec<OptionChainEvent>, Error>;

    /// Returns the sequence number of the last entry, or `None` if empty.
    #[must_use]
    fn last_sequence(&self) -> Option<u64>;
}

/// In-memory journal for testing and lightweight usage.
///
/// Stores events in a `Mutex<Vec<OptionChainEvent>>`. Not suitable for
/// production persistence but useful for unit tests and replay validation.
#[derive(Debug, Default)]
pub struct InMemoryOptionChainJournal {
    /// Stored events in sequence order.
    events: std::sync::Mutex<Vec<OptionChainEvent>>,
}

impl InMemoryOptionChainJournal {
    /// Creates a new empty in-memory journal.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the number of stored events.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.lock().map(|g| g.len()).unwrap_or(0)
    }

    /// Returns `true` if the journal contains no events.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl OptionChainJournal for InMemoryOptionChainJournal {
    fn append(&self, event: &OptionChainEvent) -> Result<(), Error> {
        let mut guard = self
            .events
            .lock()
            .map_err(|e| Error::journal_error(format!("lock poisoned: {}", e)))?;
        guard.push(event.clone());
        Ok(())
    }

    fn read_from(&self, sequence: u64) -> Result<Vec<OptionChainEvent>, Error> {
        let guard = self
            .events
            .lock()
            .map_err(|e| Error::journal_error(format!("lock poisoned: {}", e)))?;
        Ok(guard
            .iter()
            .filter(|e| e.sequence_num >= sequence)
            .cloned()
            .collect())
    }

    fn last_sequence(&self) -> Option<u64> {
        self.events
            .lock()
            .ok()
            .and_then(|g| g.last().map(|e| e.sequence_num))
    }
}

/// Internal sequencer that assigns sequence numbers and timestamps.
pub(crate) struct OptionChainSequencer {
    /// Next sequence number to assign.
    sequence: AtomicU64,
    /// Count of successfully executed commands.
    success_count: AtomicU64,
    /// Count of rejected commands.
    reject_count: AtomicU64,
}

impl OptionChainSequencer {
    /// Creates a new sequencer starting from sequence 0.
    #[must_use]
    pub fn new() -> Self {
        Self {
            sequence: AtomicU64::new(0),
            success_count: AtomicU64::new(0),
            reject_count: AtomicU64::new(0),
        }
    }

    /// Creates a new sequencer starting from a specific sequence number.
    ///
    /// Use this when resuming from a journal checkpoint.
    #[must_use]
    #[allow(dead_code)]
    pub fn with_start_sequence(start: u64) -> Self {
        Self {
            sequence: AtomicU64::new(start),
            success_count: AtomicU64::new(0),
            reject_count: AtomicU64::new(0),
        }
    }

    /// Returns the sequence number that will be assigned by the next call to `assign()`.
    #[must_use]
    #[inline]
    pub fn current_sequence(&self) -> u64 {
        self.sequence.load(Ordering::Relaxed)
    }

    /// Returns the count of successfully executed commands.
    #[must_use]
    #[inline]
    pub fn success_count(&self) -> u64 {
        self.success_count.load(Ordering::Relaxed)
    }

    /// Returns the count of rejected commands.
    #[must_use]
    #[inline]
    pub fn reject_count(&self) -> u64 {
        self.reject_count.load(Ordering::Relaxed)
    }

    /// Assigns a sequence number and timestamp to a command.
    ///
    /// Returns `(sequence_num, timestamp_ns)`.
    #[inline]
    pub fn assign(&self) -> (u64, u64) {
        let seq = self.sequence.fetch_add(1, Ordering::Relaxed);
        let ts = Self::current_time_ns();
        (seq, ts)
    }

    /// Records a successful execution.
    #[inline]
    pub fn record_success(&self) {
        self.success_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a rejected command.
    #[inline]
    pub fn record_reject(&self) {
        self.reject_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Returns the current time in nanoseconds since Unix epoch.
    #[inline]
    fn current_time_ns() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
    }
}

impl Default for OptionChainSequencer {
    fn default() -> Self {
        Self::new()
    }
}

/// A sequenced underlying order book with deterministic ordering and journaling.
///
/// Wraps an [`UnderlyingOrderBook`] and routes all operations through an
/// internal sequencer, assigning monotonic sequence numbers and optionally
/// persisting events to a journal for replay.
///
/// # Example
///
/// ```rust,ignore
/// use option_chain_orderbook::orderbook::SequencedUnderlyingOrderBook;
///
/// let book = SequencedUnderlyingOrderBook::new("BTC");
///
/// // Submit a sequenced command
/// let receipt = book.submit_add_order(
///     "BTC-20240329-50000-C",
///     order_id,
///     Side::Buy,
///     price,
///     quantity,
/// )?;
///
/// println!("Sequence: {}", receipt.sequence_num);
/// ```
pub struct SequencedUnderlyingOrderBook {
    /// The underlying order book hierarchy.
    inner: UnderlyingOrderBook,
    /// The sequencer for assigning sequence numbers.
    sequencer: OptionChainSequencer,
    /// Optional journal for event persistence.
    journal: Option<Arc<dyn OptionChainJournal>>,
}

impl SequencedUnderlyingOrderBook {
    /// Creates a new sequenced underlying order book without journaling.
    #[must_use]
    pub fn new(underlying: impl Into<String>) -> Self {
        Self {
            inner: UnderlyingOrderBook::new(underlying),
            sequencer: OptionChainSequencer::new(),
            journal: None,
        }
    }

    /// Creates a new sequenced underlying order book with a journal.
    #[must_use]
    pub fn with_journal(
        underlying: impl Into<String>,
        journal: Arc<dyn OptionChainJournal>,
    ) -> Self {
        Self {
            inner: UnderlyingOrderBook::new(underlying),
            sequencer: OptionChainSequencer::new(),
            journal: Some(journal),
        }
    }

    /// Creates a sequenced wrapper around an existing underlying order book.
    #[must_use]
    pub fn from_underlying(underlying: UnderlyingOrderBook) -> Self {
        Self {
            inner: underlying,
            sequencer: OptionChainSequencer::new(),
            journal: None,
        }
    }

    /// Creates a sequenced wrapper with journal around an existing book.
    #[must_use]
    pub fn from_underlying_with_journal(
        underlying: UnderlyingOrderBook,
        journal: Arc<dyn OptionChainJournal>,
    ) -> Self {
        Self {
            inner: underlying,
            sequencer: OptionChainSequencer::new(),
            journal: Some(journal),
        }
    }

    /// Returns a reference to the underlying order book.
    #[must_use]
    #[inline]
    pub fn underlying(&self) -> &UnderlyingOrderBook {
        &self.inner
    }

    /// Returns the current sequence number.
    #[must_use]
    #[inline]
    pub fn current_sequence(&self) -> u64 {
        self.sequencer.current_sequence()
    }

    /// Returns the count of successfully executed commands.
    #[must_use]
    #[inline]
    pub fn success_count(&self) -> u64 {
        self.sequencer.success_count()
    }

    /// Returns the count of rejected commands.
    #[must_use]
    #[inline]
    pub fn reject_count(&self) -> u64 {
        self.sequencer.reject_count()
    }

    /// Returns true if journaling is enabled.
    #[must_use]
    #[inline]
    pub fn has_journal(&self) -> bool {
        self.journal.is_some()
    }

    // ── Sequenced Operations ─────────────────────────────────────────────

    /// Submits a command and returns a receipt with the result.
    ///
    /// The command is assigned a sequence number and timestamp, executed
    /// against the underlying order book, and optionally persisted to the
    /// journal.
    ///
    /// # Errors
    ///
    /// Returns an error if journaling fails.
    pub fn submit(&self, command: OptionChainCommand) -> Result<OptionChainReceipt, Error> {
        let (seq, ts) = self.sequencer.assign();
        let result = self.execute_command(&command);

        // Record metrics
        if result.is_success() {
            self.sequencer.record_success();
        } else {
            self.sequencer.record_reject();
        }

        // Persist to journal if enabled
        if let Some(ref journal) = self.journal {
            let event = OptionChainEvent {
                sequence_num: seq,
                timestamp_ns: ts,
                command,
                result: result.clone(),
            };
            journal.append(&event)?;
        }

        Ok(OptionChainReceipt {
            sequence_num: seq,
            timestamp_ns: ts,
            result,
        })
    }

    /// Submits an add order command to a specific option book.
    ///
    /// # Arguments
    ///
    /// * `symbol` - Target option symbol (e.g., "BTC-20240329-50000-C")
    /// * `order_id` - Unique order identifier
    /// * `side` - Buy or Sell
    /// * `price` - Limit price
    /// * `quantity` - Order quantity
    ///
    /// # Errors
    ///
    /// Returns an error if the book is not found or journaling fails.
    pub fn submit_add_order(
        &self,
        symbol: &str,
        order_id: OrderId,
        side: Side,
        price: u128,
        quantity: u64,
    ) -> Result<OptionChainReceipt, Error> {
        let command = OptionChainCommand::AddOrder {
            symbol: symbol.to_string(),
            order_id,
            side,
            price,
            quantity,
        };
        self.submit(command)
    }

    /// Submits a cancel order command.
    ///
    /// # Arguments
    ///
    /// * `symbol` - Target option symbol
    /// * `order_id` - Order to cancel
    ///
    /// # Errors
    ///
    /// Returns an error if journaling fails.
    pub fn submit_cancel_order(
        &self,
        symbol: &str,
        order_id: OrderId,
    ) -> Result<OptionChainReceipt, Error> {
        let command = OptionChainCommand::CancelOrder {
            symbol: symbol.to_string(),
            order_id,
        };
        self.submit(command)
    }

    /// Submits a mass cancel command.
    ///
    /// # Arguments
    ///
    /// * `scope` - Hierarchy level for cancellation
    /// * `cancel_type` - Type of cancellation
    ///
    /// # Errors
    ///
    /// Returns an error if journaling fails.
    pub fn submit_mass_cancel(
        &self,
        scope: MassCancelScope,
        cancel_type: MassCancelType,
    ) -> Result<OptionChainReceipt, Error> {
        let command = OptionChainCommand::MassCancel { scope, cancel_type };
        self.submit(command)
    }

    // ── Command Execution ────────────────────────────────────────────────

    /// Executes a command against the underlying order book.
    fn execute_command(&self, command: &OptionChainCommand) -> OptionChainResult {
        match command {
            OptionChainCommand::AddOrder {
                symbol,
                order_id,
                side,
                price,
                quantity,
            } => self.execute_add_order(symbol, *order_id, *side, *price, *quantity),
            OptionChainCommand::CancelOrder { symbol, order_id } => {
                self.execute_cancel_order(symbol, *order_id)
            }
            OptionChainCommand::MassCancel { scope, cancel_type } => {
                self.execute_mass_cancel(scope, cancel_type)
            }
        }
    }

    /// Executes an add order operation.
    fn execute_add_order(
        &self,
        symbol: &str,
        order_id: OrderId,
        side: Side,
        price: u128,
        quantity: u64,
    ) -> OptionChainResult {
        let book = match self.find_book_by_symbol(symbol) {
            Ok(book) => book,
            Err(_) => {
                return OptionChainResult::BookNotFound {
                    symbol: symbol.to_string(),
                };
            }
        };

        match book.add_limit_order(order_id, side, price, quantity) {
            Ok(_) => OptionChainResult::OrderAdded { order_id },
            Err(e) => OptionChainResult::Rejected {
                reason: e.to_string(),
            },
        }
    }

    /// Executes a cancel order operation.
    fn execute_cancel_order(&self, symbol: &str, order_id: OrderId) -> OptionChainResult {
        let book = match self.find_book_by_symbol(symbol) {
            Ok(book) => book,
            Err(_) => {
                return OptionChainResult::BookNotFound {
                    symbol: symbol.to_string(),
                };
            }
        };

        match book.cancel_order(order_id) {
            Ok(_) => OptionChainResult::OrderCancelled { order_id },
            Err(e) => OptionChainResult::Rejected {
                reason: e.to_string(),
            },
        }
    }

    /// Executes a mass cancel operation at the specified scope.
    fn execute_mass_cancel(
        &self,
        scope: &MassCancelScope,
        cancel_type: &MassCancelType,
    ) -> OptionChainResult {
        // Execute the mass cancel based on scope and type
        let cancelled_count: usize = match (scope, cancel_type) {
            (MassCancelScope::Underlying, MassCancelType::All) => match self.inner.cancel_all() {
                Ok(r) => r.total_cancelled(),
                Err(e) => {
                    return OptionChainResult::Rejected {
                        reason: e.to_string(),
                    };
                }
            },
            (MassCancelScope::Underlying, MassCancelType::BySide(side)) => {
                match self.inner.cancel_by_side(*side) {
                    Ok(r) => r.total_cancelled(),
                    Err(e) => {
                        return OptionChainResult::Rejected {
                            reason: e.to_string(),
                        };
                    }
                }
            }
            (MassCancelScope::Underlying, MassCancelType::ByUser(user_id)) => {
                match self.inner.cancel_by_user(*user_id) {
                    Ok(r) => r.total_cancelled(),
                    Err(e) => {
                        return OptionChainResult::Rejected {
                            reason: e.to_string(),
                        };
                    }
                }
            }
            (MassCancelScope::Expiration(expiry), MassCancelType::All) => {
                match self.inner.get_expiration(expiry) {
                    Ok(exp) => match exp.cancel_all() {
                        Ok(r) => r.total_cancelled(),
                        Err(e) => {
                            return OptionChainResult::Rejected {
                                reason: e.to_string(),
                            };
                        }
                    },
                    Err(e) => {
                        return OptionChainResult::Rejected {
                            reason: e.to_string(),
                        };
                    }
                }
            }
            (MassCancelScope::Expiration(expiry), MassCancelType::BySide(side)) => {
                match self.inner.get_expiration(expiry) {
                    Ok(exp) => match exp.cancel_by_side(*side) {
                        Ok(r) => r.total_cancelled(),
                        Err(e) => {
                            return OptionChainResult::Rejected {
                                reason: e.to_string(),
                            };
                        }
                    },
                    Err(e) => {
                        return OptionChainResult::Rejected {
                            reason: e.to_string(),
                        };
                    }
                }
            }
            (MassCancelScope::Expiration(expiry), MassCancelType::ByUser(user_id)) => {
                match self.inner.get_expiration(expiry) {
                    Ok(exp) => match exp.cancel_by_user(*user_id) {
                        Ok(r) => r.total_cancelled(),
                        Err(e) => {
                            return OptionChainResult::Rejected {
                                reason: e.to_string(),
                            };
                        }
                    },
                    Err(e) => {
                        return OptionChainResult::Rejected {
                            reason: e.to_string(),
                        };
                    }
                }
            }
            (MassCancelScope::Strike { expiration, strike }, MassCancelType::All) => {
                match self.inner.get_expiration(expiration) {
                    Ok(exp) => match exp.get_strike(*strike) {
                        Ok(s) => {
                            let call_count = s
                                .call()
                                .cancel_all()
                                .map(|r| r.cancelled_count())
                                .unwrap_or(0);
                            let put_count = s
                                .put()
                                .cancel_all()
                                .map(|r| r.cancelled_count())
                                .unwrap_or(0);
                            call_count.saturating_add(put_count)
                        }
                        Err(e) => {
                            return OptionChainResult::Rejected {
                                reason: e.to_string(),
                            };
                        }
                    },
                    Err(e) => {
                        return OptionChainResult::Rejected {
                            reason: e.to_string(),
                        };
                    }
                }
            }
            (MassCancelScope::Strike { expiration, strike }, MassCancelType::BySide(side)) => {
                match self.inner.get_expiration(expiration) {
                    Ok(exp) => match exp.get_strike(*strike) {
                        Ok(s) => {
                            let call_count = s
                                .call()
                                .cancel_by_side(*side)
                                .map(|r| r.cancelled_count())
                                .unwrap_or(0);
                            let put_count = s
                                .put()
                                .cancel_by_side(*side)
                                .map(|r| r.cancelled_count())
                                .unwrap_or(0);
                            call_count.saturating_add(put_count)
                        }
                        Err(e) => {
                            return OptionChainResult::Rejected {
                                reason: e.to_string(),
                            };
                        }
                    },
                    Err(e) => {
                        return OptionChainResult::Rejected {
                            reason: e.to_string(),
                        };
                    }
                }
            }
            (MassCancelScope::Strike { expiration, strike }, MassCancelType::ByUser(user_id)) => {
                match self.inner.get_expiration(expiration) {
                    Ok(exp) => match exp.get_strike(*strike) {
                        Ok(s) => {
                            let call_count = s
                                .call()
                                .cancel_by_user(*user_id)
                                .map(|r| r.cancelled_count())
                                .unwrap_or(0);
                            let put_count = s
                                .put()
                                .cancel_by_user(*user_id)
                                .map(|r| r.cancelled_count())
                                .unwrap_or(0);
                            call_count.saturating_add(put_count)
                        }
                        Err(e) => {
                            return OptionChainResult::Rejected {
                                reason: e.to_string(),
                            };
                        }
                    },
                    Err(e) => {
                        return OptionChainResult::Rejected {
                            reason: e.to_string(),
                        };
                    }
                }
            }
            (MassCancelScope::Book(symbol), MassCancelType::All) => {
                match self.find_book_by_symbol(symbol) {
                    Ok(book) => book.cancel_all().map(|r| r.cancelled_count()).unwrap_or(0),
                    Err(e) => {
                        return OptionChainResult::Rejected {
                            reason: e.to_string(),
                        };
                    }
                }
            }
            (MassCancelScope::Book(symbol), MassCancelType::BySide(side)) => {
                match self.find_book_by_symbol(symbol) {
                    Ok(book) => book
                        .cancel_by_side(*side)
                        .map(|r| r.cancelled_count())
                        .unwrap_or(0),
                    Err(e) => {
                        return OptionChainResult::Rejected {
                            reason: e.to_string(),
                        };
                    }
                }
            }
            (MassCancelScope::Book(symbol), MassCancelType::ByUser(user_id)) => {
                match self.find_book_by_symbol(symbol) {
                    Ok(book) => book
                        .cancel_by_user(*user_id)
                        .map(|r| r.cancelled_count())
                        .unwrap_or(0),
                    Err(e) => {
                        return OptionChainResult::Rejected {
                            reason: e.to_string(),
                        };
                    }
                }
            }
        };

        // Return success with the count
        OptionChainResult::MassCancelled { cancelled_count }
    }

    /// Finds an option book by symbol.
    fn find_book_by_symbol(
        &self,
        symbol: &str,
    ) -> Result<Arc<crate::orderbook::OptionOrderBook>, Error> {
        // Fallback: parse symbol and navigate
        let parts: Vec<&str> = symbol.split('-').collect();
        if parts.len() != 4 {
            return Err(Error::invalid_symbol(
                symbol,
                "expected format: UNDERLYING-YYYYMMDD-STRIKE-C|P",
            ));
        }

        let expiry_str = parts[1];
        let strike_str = parts[2];
        let option_type = parts[3];

        // Parse expiration date
        let expiry = Self::parse_expiration(expiry_str)?;

        // Parse strike
        let strike: u64 = strike_str.parse().map_err(|_| {
            Error::invalid_symbol(symbol, format!("invalid strike: {}", strike_str))
        })?;

        let exp_book = self.inner.get_expiration(&expiry)?;
        let strike_book = exp_book.get_strike(strike)?;

        let book = match option_type.to_uppercase().as_str() {
            "C" => strike_book.call_arc(),
            "P" => strike_book.put_arc(),
            _ => {
                return Err(Error::invalid_symbol(
                    symbol,
                    format!("expected C or P, got {}", option_type),
                ));
            }
        };

        Ok(book)
    }

    /// Parses an expiration date string (YYYYMMDD format).
    fn parse_expiration(s: &str) -> Result<ExpirationDate, Error> {
        use chrono::{NaiveDate, TimeZone, Utc};

        let date = NaiveDate::parse_from_str(s, "%Y%m%d")
            .map_err(|_| Error::invalid_symbol(s, "expected YYYYMMDD format"))?;

        // Construct a concrete expiration DateTime at midnight UTC on the given date,
        // to match the representation used elsewhere in the codebase.
        let naive_dt = date
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| Error::invalid_symbol(s, "invalid expiration date"))?;
        let datetime_utc = Utc.from_utc_datetime(&naive_dt);

        Ok(ExpirationDate::DateTime(datetime_utc))
    }

    /// Replays events from the journal starting at `from_sequence`.
    ///
    /// Each event's command is re-executed against the underlying order
    /// book, and the sequencer is advanced past the highest replayed
    /// sequence number so that new commands receive non-conflicting ids.
    ///
    /// Returns the number of events replayed.
    ///
    /// # Errors
    ///
    /// Returns an error if the journal cannot be read.
    pub fn replay(&self, from_sequence: u64) -> Result<usize, Error> {
        let journal = self
            .journal
            .as_ref()
            .ok_or_else(|| Error::journal_error("replay requires a journal"))?;

        let events = journal.read_from(from_sequence)?;
        let count = events.len();

        for event in &events {
            // Re-execute command (result is discarded — we trust journal state)
            let _ = self.execute_command(&event.command);

            // Advance sequencer past the replayed range
            let next = event.sequence_num.saturating_add(1);
            self.sequencer.sequence.fetch_max(next, Ordering::Relaxed);
        }

        Ok(count)
    }

    // ── Delegated Read Operations ────────────────────────────────────────

    /// Returns the underlying symbol.
    #[must_use]
    #[inline]
    pub fn underlying_symbol(&self) -> &str {
        self.inner.underlying()
    }

    /// Returns the number of expirations.
    #[must_use]
    #[inline]
    pub fn expiration_count(&self) -> usize {
        self.inner.expiration_count()
    }

    /// Returns the total order count.
    #[must_use]
    #[inline]
    pub fn total_order_count(&self) -> usize {
        self.inner.total_order_count()
    }

    /// Returns a summary of terminal order transitions.
    #[must_use]
    #[inline]
    pub fn terminal_order_summary(&self) -> TerminalOrderSummary {
        self.inner.terminal_order_summary()
    }
}

impl std::fmt::Debug for SequencedUnderlyingOrderBook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SequencedUnderlyingOrderBook")
            .field("underlying", &self.inner.underlying())
            .field("sequence", &self.sequencer.current_sequence())
            .field("has_journal", &self.journal.is_some())
            .finish()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sequencer_assigns_monotonic_sequence() {
        let seq = OptionChainSequencer::new();
        let (s1, _) = seq.assign();
        let (s2, _) = seq.assign();
        let (s3, _) = seq.assign();

        assert_eq!(s1, 0);
        assert_eq!(s2, 1);
        assert_eq!(s3, 2);
    }

    #[test]
    fn test_sequencer_with_start_sequence() {
        let seq = OptionChainSequencer::with_start_sequence(100);
        let (s1, _) = seq.assign();
        let (s2, _) = seq.assign();

        assert_eq!(s1, 100);
        assert_eq!(s2, 101);
    }

    #[test]
    fn test_sequencer_metrics() {
        let seq = OptionChainSequencer::new();

        seq.record_success();
        seq.record_success();
        seq.record_reject();

        assert_eq!(seq.success_count(), 2);
        assert_eq!(seq.reject_count(), 1);
    }

    #[test]
    fn test_option_chain_result_is_error() {
        let success = OptionChainResult::OrderAdded {
            order_id: OrderId::new(),
        };
        let rejected = OptionChainResult::Rejected {
            reason: "test".to_string(),
        };
        let not_found = OptionChainResult::BookNotFound {
            symbol: "BTC".to_string(),
        };

        assert!(!success.is_error());
        assert!(rejected.is_error());
        assert!(not_found.is_error());
    }

    #[test]
    fn test_sequenced_book_creation() {
        let book = SequencedUnderlyingOrderBook::new("BTC");

        assert_eq!(book.underlying_symbol(), "BTC");
        assert_eq!(book.current_sequence(), 0);
        assert!(!book.has_journal());
    }

    #[test]
    fn test_mass_cancel_scope_serialization() {
        let scope = MassCancelScope::Underlying;
        let json = serde_json::to_string(&scope).expect("serialize");
        assert!(json.contains("Underlying"));
    }

    // ── InMemoryOptionChainJournal ──────────────────────────────────────

    #[test]
    fn test_in_memory_journal_empty() {
        let journal = InMemoryOptionChainJournal::new();
        assert!(journal.is_empty());
        assert_eq!(journal.len(), 0);
        assert_eq!(journal.last_sequence(), None);
    }

    #[test]
    fn test_in_memory_journal_append_and_read() {
        let journal = InMemoryOptionChainJournal::new();

        let event = OptionChainEvent {
            sequence_num: 0,
            timestamp_ns: 1000,
            command: OptionChainCommand::CancelOrder {
                symbol: "BTC-20240329-50000-C".to_string(),
                order_id: OrderId::new(),
            },
            result: OptionChainResult::Rejected {
                reason: "test".to_string(),
            },
        };

        journal.append(&event).expect("append");
        assert_eq!(journal.len(), 1);
        assert_eq!(journal.last_sequence(), Some(0));

        let events = journal.read_from(0).expect("read");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].sequence_num, 0);
    }

    #[test]
    fn test_in_memory_journal_read_from_filters() {
        let journal = InMemoryOptionChainJournal::new();

        for i in 0..5 {
            let event = OptionChainEvent {
                sequence_num: i,
                timestamp_ns: i * 1000,
                command: OptionChainCommand::CancelOrder {
                    symbol: "BTC-20240329-50000-C".to_string(),
                    order_id: OrderId::new(),
                },
                result: OptionChainResult::Rejected {
                    reason: "test".to_string(),
                },
            };
            journal.append(&event).expect("append");
        }

        assert_eq!(journal.len(), 5);
        assert_eq!(journal.last_sequence(), Some(4));

        let from_2 = journal.read_from(2).expect("read");
        assert_eq!(from_2.len(), 3); // seq 2, 3, 4

        let from_10 = journal.read_from(10).expect("read");
        assert!(from_10.is_empty());
    }

    // ── Journaled sequenced book ────────────────────────────────────────

    #[test]
    fn test_sequenced_book_with_journal() {
        let journal: Arc<dyn OptionChainJournal> = Arc::new(InMemoryOptionChainJournal::new());
        let book = SequencedUnderlyingOrderBook::with_journal("BTC", Arc::clone(&journal));

        assert!(book.has_journal());

        // Submit a command that will be rejected (no expiration exists)
        let receipt = book
            .submit_add_order("BTC-20240329-50000-C", OrderId::new(), Side::Buy, 100, 10)
            .expect("submit");

        assert!(receipt.result.is_error()); // book not found
        assert_eq!(receipt.sequence_num, 0);

        // Event should be in the journal
        assert_eq!(journal.last_sequence(), Some(0));
        let events = journal.read_from(0).expect("read");
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn test_replay_without_journal_errors() {
        let book = SequencedUnderlyingOrderBook::new("BTC");
        let result = book.replay(0);
        assert!(result.is_err());
    }

    #[test]
    fn test_replay_empty_journal() {
        let journal: Arc<dyn OptionChainJournal> = Arc::new(InMemoryOptionChainJournal::new());
        let book = SequencedUnderlyingOrderBook::with_journal("BTC", Arc::clone(&journal));

        let count = book.replay(0).expect("replay");
        assert_eq!(count, 0);
    }

    #[test]
    fn test_replay_advances_sequence() {
        let journal: Arc<dyn OptionChainJournal> = Arc::new(InMemoryOptionChainJournal::new());

        // Pre-populate journal with events
        for i in 0..3 {
            let event = OptionChainEvent {
                sequence_num: i,
                timestamp_ns: i * 1000,
                command: OptionChainCommand::CancelOrder {
                    symbol: "BTC-20240329-50000-C".to_string(),
                    order_id: OrderId::new(),
                },
                result: OptionChainResult::BookNotFound {
                    symbol: "BTC-20240329-50000-C".to_string(),
                },
            };
            journal.append(&event).expect("append");
        }

        let book = SequencedUnderlyingOrderBook::with_journal("BTC", Arc::clone(&journal));
        assert_eq!(book.current_sequence(), 0);

        let count = book.replay(0).expect("replay");
        assert_eq!(count, 3);

        // Sequence should be advanced past the replayed range
        assert!(book.current_sequence() >= 3);
    }

    #[test]
    fn test_event_serialization_roundtrip() {
        let event = OptionChainEvent {
            sequence_num: 42,
            timestamp_ns: 1_000_000,
            command: OptionChainCommand::AddOrder {
                symbol: "BTC-20240329-50000-C".to_string(),
                order_id: OrderId::new(),
                side: Side::Buy,
                price: 100,
                quantity: 10,
            },
            result: OptionChainResult::OrderAdded {
                order_id: OrderId::new(),
            },
        };

        let json = serde_json::to_string(&event).expect("serialize");
        let deserialized: OptionChainEvent = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.sequence_num, 42);
        assert_eq!(deserialized.timestamp_ns, 1_000_000);
    }
}
