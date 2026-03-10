//! Benchmarks for [`GreeksAggregator`].

use criterion::{BenchmarkId, Criterion};
use option_chain_orderbook::orderbook::greeks_aggregator::{GreeksAggregator, Position};
use optionstratlib::greeks::Greek;
use rust_decimal::Decimal;
use std::hint::black_box;

/// Creates a `Greek` with all fields set to `0.05`.
fn sample_greek() -> Greek {
    let v = Decimal::new(5, 2);
    Greek {
        delta: v,
        gamma: v,
        theta: v,
        vega: v,
        rho: v,
        rho_d: v,
        alpha: v,
        vanna: v,
        vomma: v,
        veta: v,
        charm: v,
        color: v,
    }
}

/// Populates an aggregator with `n` positions across a single account.
fn populated_aggregator(n: usize) -> GreeksAggregator {
    let agg = GreeksAggregator::new();
    let greeks = sample_greek();
    for i in 0..n {
        let symbol = format!("INST-{}", i);
        let qty = if i % 2 == 0 { 10 } else { -5 };
        agg.add_position(
            "bench-account",
            Position::new(symbol, "BTC", qty, greeks.clone()),
        );
    }
    agg
}

/// Benchmarks aggregation of N positions by account.
pub fn greeks_aggregator_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("greeks_aggregator");

    for size in [100, 500, 1000, 5000] {
        let agg = populated_aggregator(size);

        group.bench_with_input(
            BenchmarkId::new("aggregate_by_account", size),
            &size,
            |b, _| {
                b.iter(|| {
                    black_box(agg.aggregate_by_account("bench-account"));
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("aggregate_by_underlying", size),
            &size,
            |b, _| {
                b.iter(|| {
                    black_box(agg.aggregate_by_underlying("BTC"));
                });
            },
        );

        group.bench_with_input(BenchmarkId::new("aggregate_all", size), &size, |b, _| {
            b.iter(|| {
                black_box(agg.aggregate_all());
            });
        });
    }

    // Bench add_position
    group.bench_function("add_position", |b| {
        let greeks = sample_greek();
        b.iter(|| {
            let agg = GreeksAggregator::new();
            let symbol = String::from("INST-0");
            agg.add_position(
                "bench-account",
                Position::new(symbol, "BTC", 10, greeks.clone()),
            );
        });
    });

    group.finish();
}
