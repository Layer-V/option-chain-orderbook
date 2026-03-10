//! NATS JetStream integration for option chain events.
//!
//! This module provides NATS event publishing for the option chain hierarchy,
//! with hierarchical subjects encoding the full instrument path. Events are
//! published to subjects like:
//!
//! - `{prefix}.trades.{underlying}.{expiry}.{strike}.{type}`
//! - `{prefix}.book.{underlying}.{expiry}.{strike}.{type}`
//!
//! Subscribers can use NATS wildcards to filter by any level:
//!
//! - `optionchain.trades.BTC.>` — all BTC option trades
//! - `optionchain.book.ETH.20240329.>` — all ETH March 2024 book changes
//! - `optionchain.trades.*.*.50000.C` — all 50000-strike calls across underlyings
//!
//! # Feature Gate
//!
//! This module is only available when the `nats` feature is enabled:
//!
//! ```toml
//! [dependencies]
//! option-chain-orderbook = { version = "0.4", features = ["nats"] }
//! ```
//!
//! # Example
//!
//! ```rust,no_run
//! use option_chain_orderbook::orderbook::nats::OptionChainNatsConfig;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let client = async_nats::connect("nats://localhost:4222").await?;
//! let jetstream = async_nats::jetstream::new(client);
//! let handle = tokio::runtime::Handle::current();
//!
//! let config = OptionChainNatsConfig::new(jetstream, "optionchain".to_string(), handle);
//! // Use config.connect() on any hierarchy level to wire up publishers
//! # Ok(())
//! # }
//! ```

use crate::error::Error;

/// Configuration for connecting NATS publishers to the option chain hierarchy.
///
/// This struct holds the JetStream context, subject prefix, and Tokio runtime
/// handle needed to create NATS publishers at any level of the hierarchy.
///
/// # Subject Format
///
/// Events are published with hierarchical subjects:
///
/// - Trades: `{prefix}.trades.{underlying}.{expiry}.{strike}.{type}`
/// - Book changes: `{prefix}.book.{underlying}.{expiry}.{strike}.{type}`
///
/// Where:
/// - `prefix` is the configured subject prefix (e.g., `"optionchain"`)
/// - `underlying` is the underlying asset symbol (e.g., `"BTC"`)
/// - `expiry` is the expiration date in YYYYMMDD format (e.g., `"20240329"`)
/// - `strike` is the strike price (e.g., `"50000"`)
/// - `type` is `"C"` for call or `"P"` for put
#[derive(Clone)]
pub struct OptionChainNatsConfig {
    /// JetStream context for publishing messages.
    jetstream: async_nats::jetstream::Context,

    /// Subject prefix (e.g., `"optionchain"`).
    subject_prefix: String,

    /// Handle to the Tokio runtime used for spawning async publish tasks.
    runtime: tokio::runtime::Handle,
}

impl OptionChainNatsConfig {
    /// Creates a new NATS configuration for option chain event publishing.
    ///
    /// # Arguments
    ///
    /// * `jetstream` - JetStream context obtained from an `async_nats` client
    /// * `subject_prefix` - prefix for all NATS subjects (e.g., `"optionchain"`)
    /// * `runtime` - handle to the Tokio runtime for spawning publish tasks
    #[inline]
    #[must_use]
    pub fn new(
        jetstream: async_nats::jetstream::Context,
        subject_prefix: String,
        runtime: tokio::runtime::Handle,
    ) -> Self {
        Self {
            jetstream,
            subject_prefix,
            runtime,
        }
    }

    /// Returns a reference to the JetStream context.
    #[must_use]
    #[inline]
    pub fn jetstream(&self) -> &async_nats::jetstream::Context {
        &self.jetstream
    }

    /// Returns the subject prefix.
    #[must_use]
    #[inline]
    pub fn subject_prefix(&self) -> &str {
        &self.subject_prefix
    }

    /// Returns the Tokio runtime handle.
    #[must_use]
    #[inline]
    pub fn runtime(&self) -> &tokio::runtime::Handle {
        &self.runtime
    }
}

impl std::fmt::Debug for OptionChainNatsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OptionChainNatsConfig")
            .field("subject_prefix", &self.subject_prefix)
            .finish_non_exhaustive()
    }
}

/// Builds hierarchical NATS subjects from option symbol components.
///
/// This struct parses an option symbol (e.g., `"BTC-20240329-50000-C"`) and
/// generates appropriate NATS subjects for trade and book change events.
///
/// # Subject Format
///
/// - Trades: `{prefix}.trades.{underlying}.{expiry}.{strike}.{type}`
/// - Book: `{prefix}.book.{underlying}.{expiry}.{strike}.{type}`
#[derive(Debug, Clone)]
pub struct OptionChainSubjectBuilder {
    /// Underlying asset symbol (e.g., `"BTC"`).
    underlying: String,
    /// Expiration date in YYYYMMDD format (e.g., `"20240329"`).
    expiry: String,
    /// Strike price as string (e.g., `"50000"`).
    strike: String,
    /// Option type: `"C"` for call, `"P"` for put.
    option_type: String,
}

impl OptionChainSubjectBuilder {
    /// Parses an option symbol into its components.
    ///
    /// Expected format: `{underlying}-{expiry}-{strike}-{type}`
    ///
    /// # Arguments
    ///
    /// * `symbol` - Option symbol (e.g., `"BTC-20240329-50000-C"`)
    ///
    /// # Errors
    ///
    /// Returns an error if the symbol does not match the expected format.
    ///
    /// # Example
    ///
    /// ```
    /// use option_chain_orderbook::orderbook::nats::OptionChainSubjectBuilder;
    ///
    /// let builder = OptionChainSubjectBuilder::from_symbol("BTC-20240329-50000-C").unwrap();
    /// assert_eq!(builder.underlying(), "BTC");
    /// assert_eq!(builder.expiry(), "20240329");
    /// assert_eq!(builder.strike(), "50000");
    /// assert_eq!(builder.option_type(), "C");
    /// ```
    pub fn from_symbol(symbol: &str) -> Result<Self, Error> {
        let parts: Vec<&str> = symbol.split('-').collect();
        if parts.len() != 4 {
            return Err(Error::invalid_symbol(
                symbol,
                format!("expected 4 parts, got {}", parts.len()),
            ));
        }

        let option_type = parts[3].to_uppercase();
        if option_type != "C" && option_type != "P" {
            return Err(Error::invalid_symbol(
                symbol,
                format!("expected C or P, got {}", parts[3]),
            ));
        }

        Ok(Self {
            underlying: parts[0].to_string(),
            expiry: parts[1].to_string(),
            strike: parts[2].to_string(),
            option_type,
        })
    }

    /// Creates a subject builder from explicit components.
    ///
    /// # Arguments
    ///
    /// * `underlying` - Underlying asset symbol
    /// * `expiry` - Expiration date (YYYYMMDD format)
    /// * `strike` - Strike price
    /// * `option_type` - `"C"` for call, `"P"` for put
    #[must_use]
    pub fn new(
        underlying: impl Into<String>,
        expiry: impl Into<String>,
        strike: impl Into<String>,
        option_type: impl Into<String>,
    ) -> Self {
        Self {
            underlying: underlying.into(),
            expiry: expiry.into(),
            strike: strike.into(),
            option_type: option_type.into(),
        }
    }

    /// Returns the underlying asset symbol.
    #[must_use]
    #[inline]
    pub fn underlying(&self) -> &str {
        &self.underlying
    }

    /// Returns the expiration date.
    #[must_use]
    #[inline]
    pub fn expiry(&self) -> &str {
        &self.expiry
    }

    /// Returns the strike price.
    #[must_use]
    #[inline]
    pub fn strike(&self) -> &str {
        &self.strike
    }

    /// Returns the option type (`"C"` or `"P"`).
    #[must_use]
    #[inline]
    pub fn option_type(&self) -> &str {
        &self.option_type
    }

    /// Builds a trade event subject.
    ///
    /// Format: `{prefix}.trades.{underlying}.{expiry}.{strike}.{type}`
    #[must_use]
    pub fn trade_subject(&self, prefix: &str) -> String {
        format!(
            "{}.trades.{}.{}.{}.{}",
            prefix, self.underlying, self.expiry, self.strike, self.option_type
        )
    }

    /// Builds a book change event subject.
    ///
    /// Format: `{prefix}.book.{underlying}.{expiry}.{strike}.{type}`
    #[must_use]
    pub fn book_subject(&self, prefix: &str) -> String {
        format!(
            "{}.book.{}.{}.{}.{}",
            prefix, self.underlying, self.expiry, self.strike, self.option_type
        )
    }

    /// Builds both trade and book subjects as a tuple.
    ///
    /// Returns `(trade_subject, book_subject)`.
    #[must_use]
    pub fn subjects(&self, prefix: &str) -> (String, String) {
        (self.trade_subject(prefix), self.book_subject(prefix))
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subject_builder_from_symbol() {
        let builder = OptionChainSubjectBuilder::from_symbol("BTC-20240329-50000-C").unwrap();
        assert_eq!(builder.underlying(), "BTC");
        assert_eq!(builder.expiry(), "20240329");
        assert_eq!(builder.strike(), "50000");
        assert_eq!(builder.option_type(), "C");
    }

    #[test]
    fn test_subject_builder_from_symbol_put() {
        let builder = OptionChainSubjectBuilder::from_symbol("ETH-20240628-3000-P").unwrap();
        assert_eq!(builder.underlying(), "ETH");
        assert_eq!(builder.expiry(), "20240628");
        assert_eq!(builder.strike(), "3000");
        assert_eq!(builder.option_type(), "P");
    }

    #[test]
    fn test_subject_builder_lowercase_type() {
        let builder = OptionChainSubjectBuilder::from_symbol("BTC-20240329-50000-c").unwrap();
        assert_eq!(builder.option_type(), "C");
    }

    #[test]
    fn test_subject_builder_invalid_parts() {
        let result = OptionChainSubjectBuilder::from_symbol("BTC-20240329-50000");
        assert!(result.is_err());
    }

    #[test]
    fn test_subject_builder_invalid_type() {
        let result = OptionChainSubjectBuilder::from_symbol("BTC-20240329-50000-X");
        assert!(result.is_err());
    }

    #[test]
    fn test_trade_subject() {
        let builder = OptionChainSubjectBuilder::from_symbol("BTC-20240329-50000-C").unwrap();
        assert_eq!(
            builder.trade_subject("optionchain"),
            "optionchain.trades.BTC.20240329.50000.C"
        );
    }

    #[test]
    fn test_book_subject() {
        let builder = OptionChainSubjectBuilder::from_symbol("ETH-20240628-3000-P").unwrap();
        assert_eq!(
            builder.book_subject("optionchain"),
            "optionchain.book.ETH.20240628.3000.P"
        );
    }

    #[test]
    fn test_subjects_tuple() {
        let builder = OptionChainSubjectBuilder::new("BTC", "20240329", "50000", "C");
        let (trade, book) = builder.subjects("oc");
        assert_eq!(trade, "oc.trades.BTC.20240329.50000.C");
        assert_eq!(book, "oc.book.BTC.20240329.50000.C");
    }
}
