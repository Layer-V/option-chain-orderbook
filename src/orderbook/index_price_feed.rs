//! Index price feed abstraction module.
//!
//! This module provides a trait-based abstraction for external price sources
//! (Chainlink, exchange feeds, etc.) that the [`MarkPriceCalculator`] consumes.
//! This decouples the mark price calculation from the specific price source
//! implementation.
//!
//! ## Overview
//!
//! The [`IndexPriceFeed`] trait defines a pluggable interface for price sources:
//! - [`latest_price`](IndexPriceFeed::latest_price): Returns the most recent price
//! - [`subscribe`](IndexPriceFeed::subscribe): Registers a callback for price updates
//! - [`source`](IndexPriceFeed::source): Identifies the price source
//!
//! ## Implementations
//!
//! - [`MockPriceFeed`]: Programmatic price injection for testing
//! - [`StaticPriceFeed`]: Fixed price that never changes (manual injection)
//!
//! ## Wiring
//!
//! Use [`wire_feed_to_calculator`] to connect a feed to a [`MarkPriceCalculator`],
//! so that every price update automatically refreshes the calculator's index price.
//!
//! ## Example
//!
//! ```
//! use std::sync::Arc;
//! use option_chain_orderbook::orderbook::{
//!     IndexPriceFeed, MockPriceFeed, MarkPriceCalculator, wire_feed_to_calculator,
//! };
//!
//! let feed = Arc::new(MockPriceFeed::new());
//! let calculator = Arc::new(MarkPriceCalculator::with_default_config());
//!
//! // Wire feed → calculator
//! wire_feed_to_calculator(feed.as_ref(), Arc::clone(&calculator));
//!
//! // Update feed — calculator receives the price automatically
//! feed.set_price(50000);
//! assert_eq!(calculator.index_price(), 50000);
//! ```

use super::mark_price::MarkPriceCalculator;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// A price update from an external source.
///
/// Carries the price value, a nanosecond-precision timestamp for
/// observability, and the identifier of the source that produced it.
///
/// ## Fields
///
/// - `price`: Price in smallest units (e.g., satoshis, cents)
/// - `timestamp_ns`: Timestamp in nanoseconds since Unix epoch
/// - `source`: Human-readable source identifier (e.g., `"chainlink"`, `"binance"`)
#[derive(Debug, Clone)]
pub struct PriceUpdate {
    /// Price in smallest units (e.g., satoshis, cents).
    pub price: u64,
    /// Timestamp in nanoseconds since Unix epoch.
    pub timestamp_ns: u64,
    /// Identifier of the price source.
    pub source: String,
}

/// Callback invoked when a new price update is available.
///
/// Receives a shared reference to the [`PriceUpdate`] to avoid cloning
/// when multiple listeners are registered.
pub type PriceUpdateListener = Arc<dyn Fn(&PriceUpdate) + Send + Sync>;

/// Trait for external index price sources.
///
/// Implementations provide a latest-price query and a pub/sub model
/// for real-time updates. The trait is object-safe so it can be stored
/// as `Arc<dyn IndexPriceFeed>`.
///
/// ## Thread Safety
///
/// All methods must be safe to call from any thread. Implementations
/// should use interior mutability (atomics, `Mutex`, etc.) as needed.
///
/// ## Example
///
/// ```
/// use option_chain_orderbook::orderbook::{IndexPriceFeed, MockPriceFeed};
///
/// let feed = MockPriceFeed::new();
/// assert!(feed.latest_price().is_none()); // no price set yet
/// assert_eq!(feed.source(), "mock");
/// ```
pub trait IndexPriceFeed: Send + Sync {
    /// Returns the most recent price update, or `None` if no price
    /// has been published yet.
    fn latest_price(&self) -> Option<PriceUpdate>;

    /// Registers a listener that will be called on every subsequent
    /// price update. Listeners are append-only — there is no unsubscribe.
    fn subscribe(&self, listener: PriceUpdateListener);

    /// Returns a human-readable identifier for this price source
    /// (e.g., `"chainlink"`, `"binance"`, `"mock"`).
    fn source(&self) -> &str;
}

// ─── MockPriceFeed ───────────────────────────────────────────────────────────

/// Mock price feed for testing.
///
/// Allows programmatic price injection via [`set_price`](Self::set_price).
/// Every call to `set_price` notifies all registered listeners and updates
/// the latest price returned by [`latest_price`](IndexPriceFeed::latest_price).
///
/// ## Thread Safety
///
/// Uses [`AtomicU64`] for the price and a [`Mutex`] for the listener list,
/// making it safe for concurrent access from multiple threads.
///
/// ## Example
///
/// ```
/// use option_chain_orderbook::orderbook::{IndexPriceFeed, MockPriceFeed};
///
/// let feed = MockPriceFeed::new();
/// feed.set_price(42000);
///
/// let update = feed.latest_price().unwrap();
/// assert_eq!(update.price, 42000);
/// assert_eq!(update.source, "mock");
/// ```
pub struct MockPriceFeed {
    /// Current price stored atomically.
    price: AtomicU64,
    /// Timestamp of the last price update in nanoseconds.
    timestamp_ns: AtomicU64,
    /// Registered listeners notified on each `set_price` call.
    listeners: Mutex<Vec<PriceUpdateListener>>,
}

impl MockPriceFeed {
    /// Creates a new mock feed with no initial price.
    #[must_use]
    pub fn new() -> Self {
        Self {
            price: AtomicU64::new(0),
            timestamp_ns: AtomicU64::new(0),
            listeners: Mutex::new(Vec::new()),
        }
    }

    /// Sets the current price and notifies all subscribers.
    ///
    /// The timestamp is recorded as the current wall-clock time in
    /// nanoseconds since Unix epoch.
    ///
    /// # Arguments
    ///
    /// * `price` - New price in smallest units
    pub fn set_price(&self, price: u64) {
        let ts = nanos_since_epoch();
        self.price.store(price, Ordering::Release);
        self.timestamp_ns.store(ts, Ordering::Release);

        let update = PriceUpdate {
            price,
            timestamp_ns: ts,
            source: "mock".to_string(),
        };

        // Notify all listeners while holding the lock briefly.
        // The lock only protects the listener list iteration, not
        // the listener execution (listeners run synchronously).
        if let Ok(listeners) = self.listeners.lock() {
            for listener in listeners.iter() {
                listener(&update);
            }
        }
    }
}

impl Default for MockPriceFeed {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for MockPriceFeed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockPriceFeed")
            .field("price", &self.price.load(Ordering::Relaxed))
            .field("timestamp_ns", &self.timestamp_ns.load(Ordering::Relaxed))
            .finish()
    }
}

impl IndexPriceFeed for MockPriceFeed {
    fn latest_price(&self) -> Option<PriceUpdate> {
        let price = self.price.load(Ordering::Acquire);
        if price == 0 {
            return None;
        }
        Some(PriceUpdate {
            price,
            timestamp_ns: self.timestamp_ns.load(Ordering::Acquire),
            source: "mock".to_string(),
        })
    }

    fn subscribe(&self, listener: PriceUpdateListener) {
        if let Ok(mut listeners) = self.listeners.lock() {
            listeners.push(listener);
        }
    }

    fn source(&self) -> &str {
        "mock"
    }
}

// ─── StaticPriceFeed ─────────────────────────────────────────────────────────

/// Static price feed that returns a fixed price.
///
/// Useful for manual price injection or deterministic testing where
/// the price should never change. Subscribers are accepted but never
/// notified since the price is immutable after construction.
///
/// ## Example
///
/// ```
/// use option_chain_orderbook::orderbook::{IndexPriceFeed, StaticPriceFeed};
///
/// let feed = StaticPriceFeed::new(50000, "manual");
///
/// let update = feed.latest_price().unwrap();
/// assert_eq!(update.price, 50000);
/// assert_eq!(update.source, "manual");
/// ```
pub struct StaticPriceFeed {
    /// The fixed price value.
    price: u64,
    /// Source identifier.
    source: String,
    /// Timestamp recorded at construction time in nanoseconds.
    timestamp_ns: u64,
    /// Listeners accepted but never called.
    _listeners: Mutex<Vec<PriceUpdateListener>>,
}

impl StaticPriceFeed {
    /// Creates a new static feed with a fixed price.
    ///
    /// # Arguments
    ///
    /// * `price` - Fixed price in smallest units
    /// * `source` - Human-readable source identifier
    #[must_use]
    pub fn new(price: u64, source: impl Into<String>) -> Self {
        Self {
            price,
            source: source.into(),
            timestamp_ns: nanos_since_epoch(),
            _listeners: Mutex::new(Vec::new()),
        }
    }
}

impl std::fmt::Debug for StaticPriceFeed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StaticPriceFeed")
            .field("price", &self.price)
            .field("source", &self.source)
            .finish()
    }
}

impl IndexPriceFeed for StaticPriceFeed {
    fn latest_price(&self) -> Option<PriceUpdate> {
        if self.price == 0 {
            return None;
        }
        Some(PriceUpdate {
            price: self.price,
            timestamp_ns: self.timestamp_ns,
            source: self.source.clone(),
        })
    }

    fn subscribe(&self, listener: PriceUpdateListener) {
        // Accept the listener to satisfy the trait contract, but never call it
        // since the price is immutable.
        if let Ok(mut listeners) = self._listeners.lock() {
            listeners.push(listener);
        }
    }

    fn source(&self) -> &str {
        &self.source
    }
}

// ─── Wiring Helper ───────────────────────────────────────────────────────────

/// Wires an [`IndexPriceFeed`] to a [`MarkPriceCalculator`].
///
/// Subscribes a listener on the feed that calls
/// [`update_index_price`](MarkPriceCalculator::update_index_price)
/// on the calculator whenever a new price arrives. Also seeds the
/// calculator with the feed's current latest price, if available.
///
/// Returns the listener so the caller can keep a reference if needed
/// (e.g., for diagnostics or future unsubscribe support).
///
/// # Arguments
///
/// * `feed` - The price feed to subscribe to
/// * `calculator` - The mark price calculator to update
///
/// # Example
///
/// ```
/// use std::sync::Arc;
/// use option_chain_orderbook::orderbook::{
///     IndexPriceFeed, MockPriceFeed, MarkPriceCalculator, wire_feed_to_calculator,
/// };
///
/// let feed = Arc::new(MockPriceFeed::new());
/// let calc = Arc::new(MarkPriceCalculator::with_default_config());
///
/// let _listener = wire_feed_to_calculator(feed.as_ref(), Arc::clone(&calc));
///
/// feed.set_price(99000);
/// assert_eq!(calc.index_price(), 99000);
/// ```
pub fn wire_feed_to_calculator(
    feed: &dyn IndexPriceFeed,
    calculator: Arc<MarkPriceCalculator>,
) -> PriceUpdateListener {
    // Seed with current price if available
    if let Some(update) = feed.latest_price() {
        calculator.update_index_price(update.price);
    }

    // Subscribe for future updates
    let listener: PriceUpdateListener = Arc::new(move |update: &PriceUpdate| {
        calculator.update_index_price(update.price);
    });

    feed.subscribe(Arc::clone(&listener));

    listener
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Returns the current wall-clock time as nanoseconds since Unix epoch.
///
/// Falls back to `0` if the system clock is unavailable or before the epoch.
#[inline]
fn nanos_since_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::thread;

    // ── PriceUpdate ──────────────────────────────────────────────────────

    #[test]
    fn test_price_update_creation() {
        let update = PriceUpdate {
            price: 50000,
            timestamp_ns: 1_000_000_000,
            source: "test".to_string(),
        };
        assert_eq!(update.price, 50000);
        assert_eq!(update.timestamp_ns, 1_000_000_000);
        assert_eq!(update.source, "test");
    }

    #[test]
    fn test_price_update_clone() {
        let update = PriceUpdate {
            price: 42000,
            timestamp_ns: 999,
            source: "clone_test".to_string(),
        };
        let cloned = update.clone();
        assert_eq!(cloned.price, 42000);
        assert_eq!(cloned.source, "clone_test");
    }

    #[test]
    fn test_price_update_debug() {
        let update = PriceUpdate {
            price: 100,
            timestamp_ns: 0,
            source: "dbg".to_string(),
        };
        let debug = format!("{:?}", update);
        assert!(debug.contains("PriceUpdate"));
        assert!(debug.contains("100"));
    }

    // ── MockPriceFeed ────────────────────────────────────────────────────

    #[test]
    fn test_mock_feed_no_initial_price() {
        let feed = MockPriceFeed::new();
        assert!(feed.latest_price().is_none());
    }

    #[test]
    fn test_mock_feed_set_price() {
        let feed = MockPriceFeed::new();
        feed.set_price(42000);

        let update = feed.latest_price().unwrap();
        assert_eq!(update.price, 42000);
        assert_eq!(update.source, "mock");
        assert!(update.timestamp_ns > 0);
    }

    #[test]
    fn test_mock_feed_set_price_overwrites() {
        let feed = MockPriceFeed::new();
        feed.set_price(100);
        feed.set_price(200);

        let update = feed.latest_price().unwrap();
        assert_eq!(update.price, 200);
    }

    #[test]
    fn test_mock_feed_subscribe_receives_updates() {
        let feed = MockPriceFeed::new();
        let received = Arc::new(AtomicU64::new(0));
        let received_clone = Arc::clone(&received);

        feed.subscribe(Arc::new(move |update: &PriceUpdate| {
            received_clone.store(update.price, Ordering::Release);
        }));

        feed.set_price(55000);
        assert_eq!(received.load(Ordering::Acquire), 55000);
    }

    #[test]
    fn test_mock_feed_multiple_subscribers() {
        let feed = MockPriceFeed::new();
        let count = Arc::new(AtomicUsize::new(0));

        for _ in 0..3 {
            let count_clone = Arc::clone(&count);
            feed.subscribe(Arc::new(move |_: &PriceUpdate| {
                count_clone.fetch_add(1, Ordering::Relaxed);
            }));
        }

        feed.set_price(100);
        assert_eq!(count.load(Ordering::Relaxed), 3);

        feed.set_price(200);
        assert_eq!(count.load(Ordering::Relaxed), 6);
    }

    #[test]
    fn test_mock_feed_source() {
        let feed = MockPriceFeed::new();
        assert_eq!(feed.source(), "mock");
    }

    #[test]
    fn test_mock_feed_default() {
        let feed = MockPriceFeed::default();
        assert!(feed.latest_price().is_none());
        assert_eq!(feed.source(), "mock");
    }

    #[test]
    fn test_mock_feed_debug() {
        let feed = MockPriceFeed::new();
        feed.set_price(123);
        let debug = format!("{:?}", feed);
        assert!(debug.contains("MockPriceFeed"));
        assert!(debug.contains("123"));
    }

    // ── StaticPriceFeed ──────────────────────────────────────────────────

    #[test]
    fn test_static_feed_returns_fixed_price() {
        let feed = StaticPriceFeed::new(50000, "manual");

        let update = feed.latest_price().unwrap();
        assert_eq!(update.price, 50000);
        assert_eq!(update.source, "manual");
        assert!(update.timestamp_ns > 0);

        // Second call returns the same price
        let update2 = feed.latest_price().unwrap();
        assert_eq!(update2.price, 50000);
    }

    #[test]
    fn test_static_feed_zero_price_returns_none() {
        let feed = StaticPriceFeed::new(0, "empty");
        assert!(feed.latest_price().is_none());
    }

    #[test]
    fn test_static_feed_subscribe_no_updates() {
        let feed = StaticPriceFeed::new(100, "static");
        let called = Arc::new(AtomicUsize::new(0));
        let called_clone = Arc::clone(&called);

        feed.subscribe(Arc::new(move |_: &PriceUpdate| {
            called_clone.fetch_add(1, Ordering::Relaxed);
        }));

        // No set_price method on StaticPriceFeed, so listener is never called
        assert_eq!(called.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_static_feed_source() {
        let feed = StaticPriceFeed::new(1, "chainlink");
        assert_eq!(feed.source(), "chainlink");
    }

    #[test]
    fn test_static_feed_debug() {
        let feed = StaticPriceFeed::new(999, "test_src");
        let debug = format!("{:?}", feed);
        assert!(debug.contains("StaticPriceFeed"));
        assert!(debug.contains("999"));
        assert!(debug.contains("test_src"));
    }

    // ── wire_feed_to_calculator ──────────────────────────────────────────

    #[test]
    fn test_wire_feed_to_calculator_propagates_updates() {
        let feed = MockPriceFeed::new();
        let calc = Arc::new(MarkPriceCalculator::with_default_config());

        let _listener = wire_feed_to_calculator(&feed, Arc::clone(&calc));

        feed.set_price(60000);
        assert_eq!(calc.index_price(), 60000);

        feed.set_price(61000);
        assert_eq!(calc.index_price(), 61000);
    }

    #[test]
    fn test_wire_feed_seeds_existing_price() {
        let feed = MockPriceFeed::new();
        feed.set_price(45000);

        let calc = Arc::new(MarkPriceCalculator::with_default_config());
        let _listener = wire_feed_to_calculator(&feed, Arc::clone(&calc));

        // Calculator should be seeded with the existing price
        assert_eq!(calc.index_price(), 45000);
    }

    #[test]
    fn test_wire_static_feed_to_calculator() {
        let feed = StaticPriceFeed::new(70000, "oracle");
        let calc = Arc::new(MarkPriceCalculator::with_default_config());

        let _listener = wire_feed_to_calculator(&feed, Arc::clone(&calc));

        // Seeded with the static price
        assert_eq!(calc.index_price(), 70000);
    }

    #[test]
    fn test_wire_feed_no_initial_price() {
        let feed = MockPriceFeed::new();
        let calc = Arc::new(MarkPriceCalculator::with_default_config());

        let _listener = wire_feed_to_calculator(&feed, Arc::clone(&calc));

        // No price set → calculator stays at 0
        assert_eq!(calc.index_price(), 0);
    }

    // ── Thread safety ────────────────────────────────────────────────────

    #[test]
    fn test_mock_feed_thread_safety() {
        let feed = Arc::new(MockPriceFeed::new());
        let total = Arc::new(AtomicUsize::new(0));
        let total_clone = Arc::clone(&total);

        feed.subscribe(Arc::new(move |_: &PriceUpdate| {
            total_clone.fetch_add(1, Ordering::Relaxed);
        }));

        let mut handles = vec![];
        for i in 0..4 {
            let feed_clone = Arc::clone(&feed);
            handles.push(thread::spawn(move || {
                for j in 0..50 {
                    feed_clone.set_price((i * 50 + j) as u64 * 100);
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // 4 threads × 50 updates = 200 notifications
        assert_eq!(total.load(Ordering::Relaxed), 200);

        // Final price is some valid value (non-deterministic which thread wins)
        assert!(feed.latest_price().is_some());
    }

    #[test]
    fn test_wire_feed_concurrent_updates() {
        let feed = Arc::new(MockPriceFeed::new());
        let calc = Arc::new(MarkPriceCalculator::with_default_config());

        let _listener = wire_feed_to_calculator(feed.as_ref(), Arc::clone(&calc));

        let mut handles = vec![];
        for i in 1..=4 {
            let feed_clone = Arc::clone(&feed);
            handles.push(thread::spawn(move || {
                for j in 1..=25 {
                    feed_clone.set_price(i * 1000 + j);
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Calculator should have received the last update
        assert!(calc.index_price() > 0);
    }

    // ── nanos_since_epoch ────────────────────────────────────────────────

    #[test]
    fn test_nanos_since_epoch_is_positive() {
        let ns = nanos_since_epoch();
        assert!(ns > 0);
    }
}
