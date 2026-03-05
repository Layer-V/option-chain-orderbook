//! Utility functions for the Option-Chain-OrderBook library.

use crate::error::{Error, Result};
use chrono::{NaiveDate, TimeZone, Utc};
use optionstratlib::{ExpirationDate, OptionStyle};

/// Formats an `ExpirationDate` as a string in `YYYYMMDD` format.
///
/// # Arguments
///
/// * `expiration` - The expiration date to format
///
/// # Returns
///
/// A string in `YYYYMMDD` format (e.g., "20251222")
///
/// # Errors
///
/// Returns an error if the date cannot be retrieved from the `ExpirationDate`.
///
/// # Examples
///
/// ```rust
/// use option_chain_orderbook::utils::format_expiration_yyyymmdd;
/// use optionstratlib::prelude::pos_or_panic;
/// use optionstratlib::ExpirationDate;
///
/// let expiration = ExpirationDate::Days(pos_or_panic!(30.0));
/// let formatted = format_expiration_yyyymmdd(&expiration)
///     .expect("format should succeed");
/// assert_eq!(formatted.len(), 8); // YYYYMMDD format
/// ```
pub fn format_expiration_yyyymmdd(expiration: &ExpirationDate) -> Result<String> {
    let date = expiration.get_date()?;
    Ok(date.format("%Y%m%d").to_string())
}

/// Parses a YYYYMMDD string into an `ExpirationDate`.
///
/// # Arguments
///
/// * `date_str` - The date string in YYYYMMDD format
/// * `symbol` - The original symbol (for error messages)
///
/// # Returns
///
/// An `ExpirationDate::DateTime` set to 23:59:59 UTC on the parsed date.
///
/// # Errors
///
/// Returns `Error::InvalidSymbol` if the date format is invalid.
pub fn parse_yyyymmdd(date_str: &str, symbol: &str) -> Result<ExpirationDate> {
    if date_str.len() != 8 {
        return Err(Error::invalid_symbol(
            symbol,
            format!("expiration must be 8 digits (YYYYMMDD), got '{}'", date_str),
        ));
    }

    if !date_str.chars().all(|c| c.is_ascii_digit()) {
        return Err(Error::invalid_symbol(
            symbol,
            format!("expiration must be numeric, got '{}'", date_str),
        ));
    }

    let naive_date = NaiveDate::parse_from_str(date_str, "%Y%m%d")
        .map_err(|_| Error::invalid_symbol(symbol, format!("invalid date '{}'", date_str)))?;

    let naive_datetime = naive_date
        .and_hms_opt(23, 59, 59)
        .expect("23:59:59 is always a valid time");
    let datetime = Utc.from_utc_datetime(&naive_datetime);

    Ok(ExpirationDate::DateTime(datetime))
}

/// Parsed components of an option symbol.
///
/// Represents a decomposed option symbol like `BTC-20260130-50000-C` with all
/// its components extracted and validated.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedSymbol {
    /// Underlying asset (e.g., "BTC").
    pub underlying: String,
    /// Expiration date.
    pub expiration: ExpirationDate,
    /// Original expiration string (YYYYMMDD format).
    pub expiration_str: String,
    /// Strike price.
    pub strike: u64,
    /// Option type (Call or Put).
    pub option_style: OptionStyle,
}

impl ParsedSymbol {
    /// Reconstructs the symbol string from parsed components.
    ///
    /// This enables round-trip verification: parse → to_symbol → compare.
    #[must_use]
    pub fn to_symbol(&self) -> String {
        let option_char = match self.option_style {
            OptionStyle::Call => "C",
            OptionStyle::Put => "P",
        };
        format!(
            "{}-{}-{}-{}",
            self.underlying, self.expiration_str, self.strike, option_char
        )
    }
}

/// Parser for option symbol strings.
///
/// Parses symbols in the format `{UNDERLYING}-{YYYYMMDD}-{STRIKE}-{C|P}`.
///
/// # Examples
///
/// ```rust
/// use option_chain_orderbook::utils::SymbolParser;
/// use optionstratlib::OptionStyle;
///
/// let parsed = SymbolParser::parse("BTC-20260130-50000-C")
///     .expect("valid symbol");
/// assert_eq!(parsed.underlying, "BTC");
/// assert_eq!(parsed.strike, 50000);
/// assert_eq!(parsed.option_style, OptionStyle::Call);
/// assert_eq!(parsed.to_symbol(), "BTC-20260130-50000-C");
/// ```
pub struct SymbolParser;

impl SymbolParser {
    /// Parses a symbol string into its components.
    ///
    /// # Arguments
    ///
    /// * `symbol` - The symbol string to parse (e.g., "BTC-20260130-50000-C")
    ///
    /// # Returns
    ///
    /// A `ParsedSymbol` containing the extracted components.
    ///
    /// # Errors
    ///
    /// Returns `Error::InvalidSymbol` if:
    /// - The symbol doesn't have exactly 4 parts separated by `-`
    /// - The underlying is empty
    /// - The expiration is not a valid YYYYMMDD date
    /// - The strike is not a valid positive integer
    /// - The option type is not `C` or `P`
    pub fn parse(symbol: &str) -> Result<ParsedSymbol> {
        let parts: Vec<&str> = symbol.split('-').collect();

        if parts.len() != 4 {
            return Err(Error::invalid_symbol(
                symbol,
                format!(
                    "expected format UNDERLYING-YYYYMMDD-STRIKE-C|P, got {} parts",
                    parts.len()
                ),
            ));
        }

        let underlying = parts[0];
        if underlying.is_empty() {
            return Err(Error::invalid_symbol(symbol, "underlying cannot be empty"));
        }

        let expiration_str = parts[1];
        let expiration = parse_yyyymmdd(expiration_str, symbol)?;

        let strike: u64 = parts[2].parse().map_err(|_| {
            Error::invalid_symbol(
                symbol,
                format!(
                    "invalid strike price '{}', expected positive integer",
                    parts[2]
                ),
            )
        })?;

        if strike == 0 {
            return Err(Error::invalid_symbol(
                symbol,
                "strike price must be positive, got 0",
            ));
        }

        let option_style = match parts[3] {
            "C" => OptionStyle::Call,
            "P" => OptionStyle::Put,
            other => {
                return Err(Error::invalid_symbol(
                    symbol,
                    format!("invalid option type '{}', expected C or P", other),
                ));
            }
        };

        Ok(ParsedSymbol {
            underlying: underlying.to_string(),
            expiration,
            expiration_str: expiration_str.to_string(),
            strike,
            option_style,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, TimeZone, Utc};
    use optionstratlib::prelude::pos_or_panic;

    #[test]
    fn test_format_expiration_yyyymmdd_days() {
        let expiration = ExpirationDate::Days(pos_or_panic!(30.0));
        let formatted = match format_expiration_yyyymmdd(&expiration) {
            Ok(f) => f,
            Err(err) => panic!("format failed: {}", err),
        };
        assert_eq!(formatted.len(), 8);
        // Should be numeric only
        assert!(formatted.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn test_format_expiration_yyyymmdd_datetime() {
        let specific_date = match Utc.with_ymd_and_hms(2025, 12, 22, 18, 30, 0) {
            chrono::LocalResult::Single(dt) => dt,
            _ => panic!("failed to create datetime"),
        };
        let expiration = ExpirationDate::DateTime(specific_date);
        let formatted = match format_expiration_yyyymmdd(&expiration) {
            Ok(f) => f,
            Err(err) => panic!("format failed: {}", err),
        };
        assert_eq!(formatted, "20251222");
    }

    // ── Symbol Parser Tests ────────────────────────────────────────────────

    #[test]
    fn test_parse_valid_call_symbol() {
        let parsed = SymbolParser::parse("BTC-20260130-50000-C").expect("should parse");
        assert_eq!(parsed.underlying, "BTC");
        assert_eq!(parsed.expiration_str, "20260130");
        assert_eq!(parsed.strike, 50000);
        assert_eq!(parsed.option_style, OptionStyle::Call);
    }

    #[test]
    fn test_parse_valid_put_symbol() {
        let parsed = SymbolParser::parse("ETH-20251222-3000-P").expect("should parse");
        assert_eq!(parsed.underlying, "ETH");
        assert_eq!(parsed.expiration_str, "20251222");
        assert_eq!(parsed.strike, 3000);
        assert_eq!(parsed.option_style, OptionStyle::Put);
    }

    #[test]
    fn test_parse_single_char_underlying() {
        let parsed = SymbolParser::parse("E-20260101-100-C").expect("should parse");
        assert_eq!(parsed.underlying, "E");
        assert_eq!(parsed.strike, 100);
    }

    #[test]
    fn test_parse_large_strike() {
        let parsed = SymbolParser::parse("BTC-20260130-1000000-P").expect("should parse");
        assert_eq!(parsed.strike, 1_000_000);
    }

    #[test]
    fn test_parse_invalid_too_few_parts() {
        let result = SymbolParser::parse("BTC-20260130-50000");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("3 parts"));
    }

    #[test]
    fn test_parse_invalid_too_many_parts() {
        let result = SymbolParser::parse("BTC-20260130-50000-C-extra");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("5 parts"));
    }

    #[test]
    fn test_parse_invalid_date_format_short() {
        let result = SymbolParser::parse("BTC-2026013-50000-C");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("8 digits"));
    }

    #[test]
    fn test_parse_invalid_date_format_non_numeric() {
        let result = SymbolParser::parse("BTC-2026013X-50000-C");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("numeric"));
    }

    #[test]
    fn test_parse_invalid_date_value() {
        let result = SymbolParser::parse("BTC-20261340-50000-C");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("invalid date"));
    }

    #[test]
    fn test_parse_invalid_strike_not_number() {
        let result = SymbolParser::parse("BTC-20260130-abc-C");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("invalid strike"));
    }

    #[test]
    fn test_parse_invalid_option_type() {
        let result = SymbolParser::parse("BTC-20260130-50000-X");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("expected C or P"));
    }

    #[test]
    fn test_parse_empty_underlying() {
        let result = SymbolParser::parse("-20260130-50000-C");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("underlying cannot be empty"));
    }

    #[test]
    fn test_roundtrip_parse_to_symbol() {
        let original = "BTC-20260130-50000-C";
        let parsed = SymbolParser::parse(original).expect("should parse");
        let reconstructed = parsed.to_symbol();
        assert_eq!(original, reconstructed);
    }

    #[test]
    fn test_roundtrip_put_symbol() {
        let original = "ETH-20251231-2500-P";
        let parsed = SymbolParser::parse(original).expect("should parse");
        assert_eq!(original, parsed.to_symbol());
    }

    #[test]
    fn test_parsed_symbol_expiration_date_correctness() {
        let parsed = SymbolParser::parse("BTC-20260130-50000-C").expect("should parse");
        let date = parsed.expiration.get_date().expect("should get date");
        assert_eq!(date.year(), 2026);
        assert_eq!(date.month(), 1);
        assert_eq!(date.day(), 30);
    }

    #[test]
    fn test_parse_yyyymmdd_valid() {
        let result = parse_yyyymmdd("20260130", "test");
        assert!(result.is_ok());
        let exp = result.expect("should parse");
        let date = exp.get_date().expect("should get date");
        assert_eq!(date.year(), 2026);
        assert_eq!(date.month(), 1);
        assert_eq!(date.day(), 30);
    }

    #[test]
    fn test_parse_yyyymmdd_invalid_length() {
        let result = parse_yyyymmdd("2026013", "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_yyyymmdd_invalid_month() {
        let result = parse_yyyymmdd("20261330", "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_invalid_strike_zero() {
        let result = SymbolParser::parse("BTC-20260130-0-C");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("strike price must be positive"));
    }
}
