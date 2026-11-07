use market_tick_abi::error_code;
use pinocchio::program_error::ProgramError;

/// Errors returned by Market Tick state transitions.
///
/// The numbers are part of the public ABI (`market_tick_abi::error_code`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum MarketTickError {
    SlotMismatch = error_code::SLOT_MISMATCH,
    NotSlotSigner = error_code::NOT_SLOT_SIGNER,
    NonMonotonic = error_code::NON_MONOTONIC,
    IntervalMismatch = error_code::INTERVAL_MISMATCH,
    CounterOverflow = error_code::COUNTER_OVERFLOW,
    InvalidTimestamp = error_code::INVALID_TIMESTAMP,
    InvalidInterval = error_code::INVALID_INTERVAL,
}

impl From<MarketTickError> for ProgramError {
    #[inline(always)]
    fn from(error: MarketTickError) -> Self {
        ProgramError::Custom(error as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_numbers_match_the_public_abi() {
        let cases = [
            (MarketTickError::SlotMismatch, error_code::SLOT_MISMATCH),
            (MarketTickError::NotSlotSigner, error_code::NOT_SLOT_SIGNER),
            (MarketTickError::NonMonotonic, error_code::NON_MONOTONIC),
            (
                MarketTickError::IntervalMismatch,
                error_code::INTERVAL_MISMATCH,
            ),
            (
                MarketTickError::CounterOverflow,
                error_code::COUNTER_OVERFLOW,
            ),
            (
                MarketTickError::InvalidTimestamp,
                error_code::INVALID_TIMESTAMP,
            ),
            (
                MarketTickError::InvalidInterval,
                error_code::INVALID_INTERVAL,
            ),
        ];
        for (error, code) in cases {
            assert_eq!(ProgramError::from(error), ProgramError::Custom(code));
        }
    }
}
