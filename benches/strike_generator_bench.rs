//! Benchmarks for strike generation operations.

use criterion::{BenchmarkId, Criterion, Throughput};
use option_chain_orderbook::orderbook::{OptionChainOrderBook, StrikeGenerator, StrikeRangeConfig};
use optionstratlib::prelude::{ExpirationDate, Positive};

/// Creates a test expiration date.
fn test_expiration() -> ExpirationDate {
    ExpirationDate::Days(Positive::THIRTY)
}

/// Benchmarks for StrikeGenerator operations.
pub fn strike_generator_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("strike_generator");

    // Benchmark generate_strikes with default config
    group.bench_function("generate_strikes_default", |b| {
        let config = StrikeRangeConfig::builder()
            .range_pct(0.10)
            .strike_interval(1000)
            .min_strikes(5)
            .max_strikes(50)
            .build()
            .expect("valid config");

        b.iter(|| StrikeGenerator::generate_strikes(50000, &config));
    });

    // Benchmark generate_strikes with 500 strikes (acceptance criteria: < 1ms)
    group.bench_function("generate_500_strikes", |b| {
        let config = StrikeRangeConfig::builder()
            .range_pct(1.0) // 100% range to get many strikes
            .strike_interval(100)
            .min_strikes(5)
            .max_strikes(500)
            .build()
            .expect("valid config");

        b.iter(|| StrikeGenerator::generate_strikes(50000, &config));
    });

    // Benchmark apply_strikes
    group.bench_function("apply_strikes_50", |b| {
        let strikes: Vec<u64> = (40000..60000).step_by(400).collect();
        assert_eq!(strikes.len(), 50);

        b.iter_batched(
            || OptionChainOrderBook::new("BTC", test_expiration()),
            |chain| StrikeGenerator::apply_strikes(&chain, &strikes),
            criterion::BatchSize::SmallInput,
        );
    });

    // Benchmark refresh_strikes end-to-end
    group.bench_function("refresh_strikes", |b| {
        let config = StrikeRangeConfig::builder()
            .range_pct(0.10)
            .strike_interval(1000)
            .min_strikes(5)
            .max_strikes(50)
            .build()
            .expect("valid config");

        b.iter_batched(
            || OptionChainOrderBook::new("BTC", test_expiration()),
            |chain| StrikeGenerator::refresh_strikes(&chain, 50000, &config),
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

/// Benchmarks for strike generation scaling.
pub fn strike_generator_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("strike_generator_scaling");

    for num_strikes in [10, 50, 100, 250, 500].iter() {
        group.throughput(Throughput::Elements(*num_strikes as u64));

        group.bench_with_input(
            BenchmarkId::new("generate_n_strikes", num_strikes),
            num_strikes,
            |b, &num_strikes| {
                // Configure to generate approximately num_strikes
                // Formula: range_pct = (num_strikes - 1) * interval / (2 * spot)
                let range_pct = ((num_strikes - 1) as f64 * 100.0) / (2.0 * 50000.0);
                let config = StrikeRangeConfig::builder()
                    .range_pct(range_pct.min(1.0))
                    .strike_interval(100)
                    .min_strikes(5)
                    .max_strikes(num_strikes)
                    .build()
                    .expect("valid config");

                b.iter(|| StrikeGenerator::generate_strikes(50000, &config));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("apply_n_strikes", num_strikes),
            num_strikes,
            |b, &num_strikes| {
                let strikes: Vec<u64> = (0..num_strikes).map(|i| 40000 + i as u64 * 100).collect();

                b.iter_batched(
                    || OptionChainOrderBook::new("BTC", test_expiration()),
                    |chain| StrikeGenerator::apply_strikes(&chain, &strikes),
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}
