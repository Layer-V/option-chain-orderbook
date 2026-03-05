//! Instrument status module.
//!
//! This module provides the [`InstrumentStatus`] enum for tracking the lifecycle
//! state of an option instrument. Only instruments with [`InstrumentStatus::Active`]
//! status accept new orders.

use serde::{Deserialize, Serialize};

/// Lifecycle status of an option instrument.
///
/// Tracks the current state of an instrument in the trading system.
/// Only [`Active`](InstrumentStatus::Active) instruments accept new orders.
///
/// ## State Transitions
///
/// ```text
/// Pending → Active → Halted → Active (resume)
///                  → Settling → Expired
///                  → Expired (direct)
/// ```
///
/// ## Thread Safety
///
/// This enum is stored as an [`AtomicU8`](std::sync::atomic::AtomicU8) inside
/// [`OptionOrderBook`](super::book::OptionOrderBook) for lock-free concurrent access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum InstrumentStatus {
    /// Instrument is pending activation (not yet trading).
    Pending = 0,
    /// Instrument is active and accepting orders.
    Active = 1,
    /// Instrument is temporarily halted (no new orders accepted).
    Halted = 2,
    /// Instrument is in settlement process (no new orders accepted).
    Settling = 3,
    /// Instrument has expired (no new orders accepted, all orders cancelled).
    Expired = 4,
}

impl InstrumentStatus {
    /// Converts a `u8` value to an `InstrumentStatus`.
    ///
    /// Returns `None` if the value does not correspond to a valid status.
    ///
    /// # Arguments
    ///
    /// * `value` - The raw `u8` value to convert
    #[must_use]
    #[inline]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Pending),
            1 => Some(Self::Active),
            2 => Some(Self::Halted),
            3 => Some(Self::Settling),
            4 => Some(Self::Expired),
            _ => None,
        }
    }

    /// Returns `true` if this status allows new orders to be placed.
    ///
    /// Only [`Active`](InstrumentStatus::Active) instruments accept orders.
    #[must_use]
    #[inline]
    pub const fn is_accepting_orders(&self) -> bool {
        matches!(self, Self::Active)
    }
}

impl std::fmt::Display for InstrumentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "Pending"),
            Self::Active => write!(f, "Active"),
            Self::Halted => write!(f, "Halted"),
            Self::Settling => write!(f, "Settling"),
            Self::Expired => write!(f, "Expired"),
        }
    }
}

impl Default for InstrumentStatus {
    /// Default status is [`Active`](InstrumentStatus::Active).
    ///
    /// Newly created order books are immediately ready to accept orders.
    fn default() -> Self {
        Self::Active
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instrument_status_display() {
        assert_eq!(InstrumentStatus::Pending.to_string(), "Pending");
        assert_eq!(InstrumentStatus::Active.to_string(), "Active");
        assert_eq!(InstrumentStatus::Halted.to_string(), "Halted");
        assert_eq!(InstrumentStatus::Settling.to_string(), "Settling");
        assert_eq!(InstrumentStatus::Expired.to_string(), "Expired");
    }

    #[test]
    fn test_instrument_status_from_u8() {
        assert_eq!(
            InstrumentStatus::from_u8(0),
            Some(InstrumentStatus::Pending)
        );
        assert_eq!(InstrumentStatus::from_u8(1), Some(InstrumentStatus::Active));
        assert_eq!(InstrumentStatus::from_u8(2), Some(InstrumentStatus::Halted));
        assert_eq!(
            InstrumentStatus::from_u8(3),
            Some(InstrumentStatus::Settling)
        );
        assert_eq!(
            InstrumentStatus::from_u8(4),
            Some(InstrumentStatus::Expired)
        );
        assert_eq!(InstrumentStatus::from_u8(5), None);
        assert_eq!(InstrumentStatus::from_u8(255), None);
    }

    #[test]
    fn test_instrument_status_from_u8_roundtrip() {
        for &status in &[
            InstrumentStatus::Pending,
            InstrumentStatus::Active,
            InstrumentStatus::Halted,
            InstrumentStatus::Settling,
            InstrumentStatus::Expired,
        ] {
            let raw = status as u8;
            assert_eq!(InstrumentStatus::from_u8(raw), Some(status));
        }
    }

    #[test]
    fn test_instrument_status_is_accepting_orders() {
        assert!(!InstrumentStatus::Pending.is_accepting_orders());
        assert!(InstrumentStatus::Active.is_accepting_orders());
        assert!(!InstrumentStatus::Halted.is_accepting_orders());
        assert!(!InstrumentStatus::Settling.is_accepting_orders());
        assert!(!InstrumentStatus::Expired.is_accepting_orders());
    }

    #[test]
    fn test_instrument_status_default() {
        assert_eq!(InstrumentStatus::default(), InstrumentStatus::Active);
    }

    #[test]
    fn test_instrument_status_clone_copy() {
        let status = InstrumentStatus::Active;
        let cloned = status;
        assert_eq!(status, cloned);
    }

    #[test]
    fn test_instrument_status_eq() {
        assert_eq!(InstrumentStatus::Active, InstrumentStatus::Active);
        assert_ne!(InstrumentStatus::Active, InstrumentStatus::Halted);
    }

    #[test]
    fn test_instrument_status_debug() {
        let debug = format!("{:?}", InstrumentStatus::Settling);
        assert_eq!(debug, "Settling");
    }

    #[test]
    fn test_instrument_status_serde_roundtrip() {
        for &status in &[
            InstrumentStatus::Pending,
            InstrumentStatus::Active,
            InstrumentStatus::Halted,
            InstrumentStatus::Settling,
            InstrumentStatus::Expired,
        ] {
            let json = match serde_json::to_string(&status) {
                Ok(j) => j,
                Err(err) => panic!("serialization failed: {}", err),
            };
            let deserialized: InstrumentStatus = match serde_json::from_str(&json) {
                Ok(d) => d,
                Err(err) => panic!("deserialization failed: {}", err),
            };
            assert_eq!(status, deserialized);
        }
    }

    #[test]
    fn test_instrument_status_hash() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(InstrumentStatus::Active);
        set.insert(InstrumentStatus::Halted);
        set.insert(InstrumentStatus::Active); // duplicate

        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_instrument_status_repr_values() {
        assert_eq!(InstrumentStatus::Pending as u8, 0);
        assert_eq!(InstrumentStatus::Active as u8, 1);
        assert_eq!(InstrumentStatus::Halted as u8, 2);
        assert_eq!(InstrumentStatus::Settling as u8, 3);
        assert_eq!(InstrumentStatus::Expired as u8, 4);
    }
}
