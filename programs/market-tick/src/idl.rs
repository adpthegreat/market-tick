//! IDL-only definitions, compiled with `--features idl` and never part of the
//! deployed binary. `shank idl` reads these to emit `idls/market_tick.json`.
//! They mirror the wire format handled by `instructions` and `state`.

use pinocchio::pubkey::Pubkey;
use shank::{ShankAccount, ShankInstruction};
use thiserror::Error;

#[repr(C, u8)]
#[derive(Clone, Debug, PartialEq, ShankInstruction)]
pub enum MarketTickInstruction {
    /// Create the canonical V1 PDA, including when it was prefunded.
    #[account(0, signer, writable, name = "payer", desc = "Funds the account")]
    #[account(
        1,
        writable,
        name = "pda",
        desc = "Canonical V1 PDA [\"market_tick\", \"v1\"]"
    )]
    #[account(2, name = "system_program", desc = "System program")]
    Initialize,

    /// Record a tick for the current slot.
    #[account(
        0,
        writable,
        name = "pda",
        desc = "Canonical V1 PDA [\"market_tick\", \"v1\"]"
    )]
    #[account(
        1,
        signer,
        name = "signer",
        desc = "Ticker; the first valid signer in a slot owns it"
    )]
    Increment {
        /// Must equal the runtime clock slot.
        slot: u64,
        /// Positive; strictly increasing within a slot.
        timestamp_ns: i64,
        /// Nonzero; fixed within a slot.
        target_market_tick_interval_ns: u64,
    },
}

/// 88-byte V1 account. Integers are little-endian.
#[repr(C)]
#[derive(Clone, Copy, Debug, ShankAccount)]
pub struct MarketTickV1 {
    /// `b"MRKTKV01"`.
    pub discriminator: [u8; 8],
    /// First accepted signer in `slot`; all zero before the first tick.
    pub signer: Pubkey,
    pub slot: u64,
    pub first_timestamp_ns: i64,
    pub timestamp_ns: i64,
    /// Zero-based count of ticks after the first one in `slot`.
    pub sequence: u64,
    pub target_market_tick_interval_ns: u64,
    /// Gap between the two most recent ticks in `slot`.
    pub observed_market_tick_interval_ns: u64,
}

#[derive(Clone, Copy, Debug, Error)]
pub enum MarketTickError {
    #[error("Instruction slot does not match the runtime clock slot")]
    SlotMismatch = 6000,
    #[error("Signer is not the signer selected for this slot")]
    NotSlotSigner = 6001,
    #[error("Timestamp must strictly increase within a slot")]
    NonMonotonic = 6002,
    #[error("Target interval cannot change within a slot")]
    IntervalMismatch = 6003,
    #[error("Sequence counter overflow")]
    CounterOverflow = 6004,
    #[error("Timestamp must be positive")]
    InvalidTimestamp = 6005,
    #[error("Target interval must be nonzero")]
    InvalidInterval = 6006,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error;

    #[test]
    fn idl_errors_match_program_errors() {
        let pairs = [
            (
                MarketTickError::SlotMismatch,
                error::MarketTickError::SlotMismatch,
            ),
            (
                MarketTickError::NotSlotSigner,
                error::MarketTickError::NotSlotSigner,
            ),
            (
                MarketTickError::NonMonotonic,
                error::MarketTickError::NonMonotonic,
            ),
            (
                MarketTickError::IntervalMismatch,
                error::MarketTickError::IntervalMismatch,
            ),
            (
                MarketTickError::CounterOverflow,
                error::MarketTickError::CounterOverflow,
            ),
            (
                MarketTickError::InvalidTimestamp,
                error::MarketTickError::InvalidTimestamp,
            ),
            (
                MarketTickError::InvalidInterval,
                error::MarketTickError::InvalidInterval,
            ),
        ];
        for (idl, program) in pairs {
            assert_eq!(idl as u32, program as u32);
        }
    }

    #[test]
    fn idl_account_matches_state_size() {
        assert_eq!(
            core::mem::size_of::<MarketTickV1>(),
            crate::state::MarketTickV1State::LEN
        );
    }
}
