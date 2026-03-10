//! Benchmarks for option-chain-orderbook library.
//!
//! This module provides comprehensive benchmarks for all order book components:
//!
//! - **orderbook_bench**: Single option order book operations
//! - **strike_bench**: Strike order book and manager operations
//! - **chain_bench**: Option chain order book and manager operations
//! - **expiration_bench**: Expiration order book and manager operations
//! - **underlying_bench**: Underlying order book and manager operations
//! - **hierarchy_bench**: Full hierarchy traversal and trading scenarios

mod chain_bench;
mod expiration_bench;
mod greeks_aggregator_bench;
mod hierarchy_bench;
mod orderbook_bench;
mod strike_bench;
mod strike_generator_bench;
mod underlying_bench;

use criterion::{criterion_group, criterion_main};

// OptionOrderBook benchmarks
criterion_group!(
    orderbook_benches,
    orderbook_bench::orderbook_operations,
    orderbook_bench::orderbook_scaling,
);

// StrikeOrderBook benchmarks
criterion_group!(
    strike_benches,
    strike_bench::strike_orderbook_operations,
    strike_bench::strike_manager_operations,
    strike_bench::strike_manager_scaling,
);

// OptionChainOrderBook benchmarks
criterion_group!(
    chain_benches,
    chain_bench::chain_orderbook_operations,
    chain_bench::chain_manager_operations,
    chain_bench::chain_manager_scaling,
);

// ExpirationOrderBook benchmarks
criterion_group!(
    expiration_benches,
    expiration_bench::expiration_orderbook_operations,
    expiration_bench::expiration_manager_operations,
    expiration_bench::expiration_manager_scaling,
);

// UnderlyingOrderBook benchmarks
criterion_group!(
    underlying_benches,
    underlying_bench::underlying_orderbook_operations,
    underlying_bench::underlying_manager_operations,
    underlying_bench::underlying_manager_scaling,
);

// Full hierarchy benchmarks
criterion_group!(
    hierarchy_benches,
    hierarchy_bench::hierarchy_operations,
    hierarchy_bench::trading_scenarios,
    hierarchy_bench::hierarchy_scaling,
);

// StrikeGenerator benchmarks
criterion_group!(
    strike_generator_benches,
    strike_generator_bench::strike_generator_operations,
    strike_generator_bench::strike_generator_scaling,
);

// GreeksAggregator benchmarks
criterion_group!(
    greeks_aggregator_benches,
    greeks_aggregator_bench::greeks_aggregator_operations,
);

criterion_main!(
    orderbook_benches,
    strike_benches,
    chain_benches,
    expiration_benches,
    underlying_benches,
    hierarchy_benches,
    strike_generator_benches,
    greeks_aggregator_benches
);
