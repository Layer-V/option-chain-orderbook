//! Utility functions for the Option-Chain-OrderBook library.

use crate::error::Result;
use optionstratlib::ExpirationDate;

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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
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
}
