//! # Option Chain Order Book - Options Market Making Infrastructure
//!
//! A high-performance Rust library for options market making infrastructure,
//! providing a complete Option Chain Order Book system built on top of
//! [OrderBook-rs](https://crates.io/crates/orderbook-rs),
//! [PriceLevel](https://crates.io/crates/pricelevel), and
//! [OptionStratLib](https://crates.io/crates/optionstratlib).
//!
//! ## Key Features
//!
//! - **Lock-Free Architecture**: Built on OrderBook-rs's lock-free data structures
//!   for maximum throughput in high-frequency trading scenarios.
//!
//! - **Hierarchical Order Book Structure**: Multi-level organization from
//!   underlying assets down to individual option contracts.
//!
//! - **Multi-Expiration Option Chain Management**: Handle hundreds of options
//!   across multiple strikes and expirations simultaneously.
//!
//! - **Real-Time Order Book per Option**: Individual order books for each option
//!   contract with full depth, powered by OrderBook-rs.
//!
//! - **Thread-Safe Concurrent Access**: Uses `SkipMap` for lock-free concurrent
//!   access to order books across multiple threads.
//!
//! - **OptionStratLib Integration**: Use Greeks calculation, `ExpirationDate`,
//!   `OptionStyle`, and pricing models directly from OptionStratLib.
//!
//! - **Result-Based Error Handling**: All fallible operations return `Result<T, Error>`
//!   with descriptive error types.
//!
//! ## Architecture
//!
//! The library follows a hierarchical structure for option chain management:
//!
//! ```text
//! UnderlyingOrderBookManager (manages all underlyings: BTC, ETH, SPX, etc.)
//!   └── UnderlyingOrderBook (per underlying, all expirations for one asset)
//!         └── ExpirationOrderBookManager (manages all expirations for underlying)
//!               └── ExpirationOrderBook (per expiry date)
//!                     └── OptionChainOrderBook (per expiration, option chain)
//!                           └── StrikeOrderBookManager (manages all strikes)
//!                                 └── StrikeOrderBook (per strike price, call/put pair)
//!                                       └── OptionOrderBook (call or put)
//!                                             └── OrderBook<T> (from OrderBook-rs)
//! ```
//!
//! This architecture enables:
//! - Efficient aggregation of Greeks and positions at any level
//! - Fast lookup of specific option contracts
//! - Scalable management of large option chains
//! - ATM strike lookup at any level
//! - Statistics aggregation across the hierarchy
//!
//! ## Module Structure
//!
//! | Module | Description |
//! |--------|-------------|
//! | [`orderbook`] | Hierarchical order book structure with all managers |
//! | [`error`] | Error types and `Result` type alias |
//! | [`utils`] | Utility functions (e.g., date formatting) |
//!
//! ## Core Components
//!
//! ### Order Book Hierarchy ([`orderbook`])
//!
//! - [`orderbook::UnderlyingOrderBookManager`]: Top-level manager for all underlyings
//! - [`orderbook::UnderlyingOrderBook`]: All expirations for a single underlying
//! - [`orderbook::ExpirationOrderBookManager`]: Manages expirations for an underlying
//! - [`orderbook::ExpirationOrderBook`]: All strikes for a single expiration
//! - [`orderbook::OptionChainOrderBook`]: Option chain with strike management
//! - [`orderbook::StrikeOrderBookManager`]: Manages strikes for an expiration
//! - [`orderbook::StrikeOrderBook`]: Call/put pair at a strike price
//! - [`orderbook::OptionOrderBook`]: Single option order book
//! - [`orderbook::Quote`]: Two-sided market representation
//! - [`orderbook::QuoteUpdate`]: Quote change tracking
//!
//! ## Example Usage
//!
//! ### Creating a Hierarchical Order Book
//!
//! ```rust
//! use option_chain_orderbook::orderbook::UnderlyingOrderBookManager;
//! use optionstratlib::prelude::pos_or_panic;
//! use optionstratlib::ExpirationDate;
//! use orderbook_rs::{OrderId, Side};
//!
//! let manager = UnderlyingOrderBookManager::new();
//! let exp_date = ExpirationDate::Days(pos_or_panic!(30.0));
//!
//! // Create BTC option chain (use block to drop guards)
//! {
//!     let btc = manager.get_or_create("BTC");
//!     let exp = btc.get_or_create_expiration(exp_date);
//!     let strike = exp.get_or_create_strike(50000);
//!
//!     // Add orders to call
//!     strike.call().add_limit_order(OrderId::new(), Side::Buy, 100, 10)
//!         .expect("add order should succeed");
//!     strike.call().add_limit_order(OrderId::new(), Side::Sell, 105, 5)
//!         .expect("add order should succeed");
//!
//!     // Get quote
//!     let quote = strike.call().best_quote();
//!     assert!(quote.is_two_sided());
//! }
//!
//! // Get statistics
//! let stats = manager.stats();
//! ```
//!
//! ### Creating a Single Option Order Book
//!
//! ```rust
//! use option_chain_orderbook::orderbook::OptionOrderBook;
//! use optionstratlib::OptionStyle;
//! use orderbook_rs::{OrderId, Side};
//!
//! // Create an order book for a specific option
//! let book = OptionOrderBook::new("BTC-20240329-50000-C", OptionStyle::Call);
//!
//! // Add limit orders
//! book.add_limit_order(OrderId::new(), Side::Buy, 500, 10)
//!     .expect("add order should succeed");
//! book.add_limit_order(OrderId::new(), Side::Sell, 520, 5)
//!     .expect("add order should succeed");
//!
//! // Get the best quote
//! let quote = book.best_quote();
//! assert!(quote.is_two_sided());
//! ```
//!
//! ### Using OptionStratLib for Greeks
//!
//! ```rust,ignore
//! use optionstratlib::prelude::pos_or_panic;
//! use optionstratlib::{Options, ExpirationDate};
//! use optionstratlib::model::types::{OptionStyle, OptionType, Side};
//! use optionstratlib::greeks::{delta, gamma, theta, vega, rho};
//! use rust_decimal_macros::dec;
//!
//! let option = Options {
//!     option_type: OptionType::European,
//!     side: Side::Long,
//!     underlying_symbol: "BTC".to_string(),
//!     strike_price: pos_or_panic!(50000.0),
//!     expiration_date: ExpirationDate::Days(pos_or_panic!(30.0)),
//!     implied_volatility: pos_or_panic!(0.6),
//!     quantity: pos_or_panic!(1.0),
//!     underlying_price: pos_or_panic!(48000.0),
//!     risk_free_rate: dec!(0.05),
//!     option_style: OptionStyle::Call,
//!     dividend_yield: pos_or_panic!(0.0),
//!     exotic_params: None,
//! };
//!
//! let delta_value = delta(&option).expect("delta calculation should succeed");
//! let gamma_value = gamma(&option).expect("gamma calculation should succeed");
//! ```
//!
//! ## Examples
//!
//! The library includes comprehensive examples demonstrating each level of the hierarchy:
//!
//! | Example | Description |
//! |---------|-------------|
//! | `01_option_orderbook` | Single option order book operations |
//! | `02_strike_orderbook` | Strike level with call/put pairs |
//! | `03_chain_orderbook` | Option chain (all strikes for one expiration) |
//! | `04_expiration_orderbook` | Expiration level with term structure |
//! | `05_underlying_orderbook` | Underlying level (all expirations) |
//! | `06_full_hierarchy` | Complete hierarchy with trading scenarios |
//! | `07_mass_cancel` | Hierarchical mass cancel operations |
//! | `08_order_lifecycle` | Order state tracking and lifecycle queries |
//!
//! Run examples with:
//! ```bash
//! cargo run --example 01_option_orderbook
//! cargo run --example 06_full_hierarchy
//! ```
//!
//! ## Benchmarks
//!
//! Comprehensive benchmarks are available for all components:
//!
//! - **orderbook_bench**: Single option order book operations
//! - **strike_bench**: Strike order book and manager operations
//! - **chain_bench**: Option chain order book operations
//! - **expiration_bench**: Expiration order book operations
//! - **underlying_bench**: Underlying order book operations
//! - **hierarchy_bench**: Full hierarchy traversal and trading scenarios
//!
//! Run benchmarks with:
//! ```bash
//! cargo bench
//! cargo bench -- orderbook_benches
//! cargo bench -- hierarchy_benches
//! ```
//!
//! ## Performance Characteristics
//!
//! Built on OrderBook-rs's lock-free architecture:
//!
//! - **Order Operations**: O(log N) for add/cancel operations
//! - **Best Quote Lookup**: O(1) with caching
//! - **Thread Safety**: Lock-free operations for concurrent access
//! - **Hierarchy Traversal**: O(log N) access via `SkipMap`
//!
//! ## Dependencies
//!
//! - **orderbook-rs** (0.6): Lock-free order book engine
//! - **optionstratlib** (0.15): Options pricing, Greeks, and strategy analysis
//! - **crossbeam-skiplist** (0.1): Lock-free concurrent skip list
//! - **rust_decimal** (1.40): Precise decimal arithmetic
//! - **thiserror** (2.0): Error handling
//! - **serde** (1.0): Serialization support

pub mod error;
pub mod orderbook;
pub mod utils;

pub use error::{Error, Result};
pub use orderbook::{
    CycleRule, ExpirationCallback, ExpiryCycleConfig, ExpiryScheduler, ExpiryType, RefreshResult,
    StrikeGenerator, StrikeRangeConfig, StrikeRangeConfigBuilder, SymbolIndex, SymbolRef,
};
pub use utils::{ParsedSymbol, SymbolParser};
