use pinocchio::{
    program_error::ProgramError,
    pubkey::{pubkey_eq, Pubkey},
    ProgramResult,
};

use super::{IncrementAccounts, IncrementData};
use crate::{error::MarketTickError, state::MarketTickV1State};

const NO_SIGNER: Pubkey = [0; 32];

/// Reads `Clock::slot` straight from the syscall into a stack slot.
#[inline(always)]
fn current_slot() -> Result<u64, ProgramError> {
    #[cfg(target_os = "solana")]
    {
        let mut clock = core::mem::MaybeUninit::<pinocchio::sysvars::clock::Clock>::uninit();
        // SAFETY: the syscall writes a full `Clock` on success; `slot` is
        // only read after a zero return.
        unsafe {
            if pinocchio::syscalls::sol_get_clock_sysvar(clock.as_mut_ptr() as *mut u8) != 0 {
                return Err(ProgramError::UnsupportedSysvar);
            }
            Ok((*clock.as_ptr()).slot)
        }
    }
    #[cfg(not(target_os = "solana"))]
    Err(ProgramError::UnsupportedSysvar)
}

#[inline(always)]
pub fn process_increment(accounts: &IncrementAccounts, data: IncrementData) -> ProgramResult {
    let current_slot = current_slot()?;
    // SAFETY: `IncrementAccounts` verified that the data region is exactly
    // `MarketTickV1State::LEN` bytes; the runtime aligns account data to 8
    // bytes; nothing else borrows the account data here.
    let state = unsafe { MarketTickV1State::from_account_mut_unchecked(&accounts.pda) };
    increment_v1(state, accounts.signer.key(), current_slot, data)
}

/// Applies one increment to `state`. On error `state` is left untouched.
#[inline(always)]
pub fn increment_v1(
    state: &mut MarketTickV1State,
    signer: &Pubkey,
    current_slot: u64,
    data: IncrementData,
) -> ProgramResult {
    let IncrementData {
        slot,
        timestamp_ns,
        target_market_tick_interval_ns,
    } = data;

    if slot != current_slot {
        return Err(MarketTickError::SlotMismatch.into());
    }
    if timestamp_ns <= 0 {
        return Err(MarketTickError::InvalidTimestamp.into());
    }
    if target_market_tick_interval_ns == 0 {
        return Err(MarketTickError::InvalidInterval.into());
    }

    if state.slot != slot || pubkey_eq(&state.signer, &NO_SIGNER) {
        state.signer = *signer;
        state.slot = slot;
        state.first_timestamp_ns = timestamp_ns;
        state.timestamp_ns = timestamp_ns;
        state.sequence = 0;
        state.target_market_tick_interval_ns = target_market_tick_interval_ns;
        state.observed_market_tick_interval_ns = 0;
        return Ok(());
    }

    if !pubkey_eq(&state.signer, signer) {
        return Err(MarketTickError::NotSlotSigner.into());
    }
    if state.target_market_tick_interval_ns != target_market_tick_interval_ns {
        return Err(MarketTickError::IntervalMismatch.into());
    }
    if timestamp_ns <= state.timestamp_ns {
        return Err(MarketTickError::NonMonotonic.into());
    }
    let Some(sequence) = state.sequence.checked_add(1) else {
        return Err(MarketTickError::CounterOverflow.into());
    };

    state.sequence = sequence;
    state.observed_market_tick_interval_ns =
        (timestamp_ns as u64).wrapping_sub(state.timestamp_ns as u64);
    state.timestamp_ns = timestamp_ns;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::MarketTickError;

    const SLOT: u64 = 42;
    const TIMESTAMP: i64 = 1_000;
    const INTERVAL: u64 = 10;

    fn signer(byte: u8) -> Pubkey {
        [byte; 32]
    }

    fn initialized() -> MarketTickV1State {
        MarketTickV1State {
            discriminator: crate::state::DISCRIMINATOR_V1_U64,
            ..Default::default()
        }
    }

    fn increment(
        state: &mut MarketTickV1State,
        signer: Pubkey,
        current_slot: u64,
        slot: u64,
        timestamp_ns: i64,
        interval_ns: u64,
    ) -> ProgramResult {
        increment_v1(
            state,
            &signer,
            current_slot,
            IncrementData {
                slot,
                timestamp_ns,
                target_market_tick_interval_ns: interval_ns,
            },
        )
    }

    fn err(error: MarketTickError) -> ProgramResult {
        Err(ProgramError::from(error))
    }

    #[test]
    fn first_increment_initializes_slot_state() {
        let mut state = initialized();
        increment(&mut state, signer(1), SLOT, SLOT, TIMESTAMP, INTERVAL).unwrap();

        assert!(state.has_valid_discriminator());
        assert_eq!(state.signer, signer(1));
        assert_eq!(state.slot, SLOT);
        assert_eq!(state.first_timestamp_ns, TIMESTAMP);
        assert_eq!(state.timestamp_ns, TIMESTAMP);
        assert_eq!(state.sequence, 0);
        assert_eq!(state.target_market_tick_interval_ns, INTERVAL);
        assert_eq!(state.observed_market_tick_interval_ns, 0);
    }

    #[test]
    fn zero_signer_in_slot_zero_counts_as_first_increment() {
        let mut state = initialized();
        increment(&mut state, signer(9), 0, 0, 5, 1).unwrap();
        assert_eq!(state.signer, signer(9));
        assert_eq!(state.sequence, 0);
        assert_eq!(state.first_timestamp_ns, 5);
    }

    #[test]
    fn new_slot_rotates_signer_and_resets_state() {
        let mut state = initialized();
        increment(&mut state, signer(1), SLOT, SLOT, TIMESTAMP, INTERVAL).unwrap();
        increment(&mut state, signer(1), SLOT, SLOT, TIMESTAMP + 7, INTERVAL).unwrap();

        increment(
            &mut state,
            signer(2),
            SLOT + 1,
            SLOT + 1,
            TIMESTAMP + 500,
            INTERVAL + 5,
        )
        .unwrap();

        assert_eq!(state.signer, signer(2));
        assert_eq!(state.slot, SLOT + 1);
        assert_eq!(state.first_timestamp_ns, TIMESTAMP + 500);
        assert_eq!(state.timestamp_ns, TIMESTAMP + 500);
        assert_eq!(state.sequence, 0);
        assert_eq!(state.target_market_tick_interval_ns, INTERVAL + 5);
        assert_eq!(state.observed_market_tick_interval_ns, 0);
    }

    #[test]
    fn new_slot_accepts_earlier_timestamp_and_any_signer() {
        let mut state = initialized();
        increment(&mut state, signer(1), SLOT, SLOT, TIMESTAMP, INTERVAL).unwrap();
        increment(&mut state, signer(3), SLOT + 1, SLOT + 1, 1, 1).unwrap();
        assert_eq!(state.timestamp_ns, 1);
        assert_eq!(state.signer, signer(3));
    }

    #[test]
    fn same_slot_increment_requires_a_strictly_increasing_timestamp() {
        let mut state = initialized();
        increment(&mut state, signer(1), SLOT, SLOT, TIMESTAMP, INTERVAL).unwrap();
        let before = state;

        assert_eq!(
            increment(&mut state, signer(1), SLOT, SLOT, TIMESTAMP, INTERVAL),
            err(MarketTickError::NonMonotonic)
        );
        assert_eq!(state, before);
        assert_eq!(
            increment(&mut state, signer(1), SLOT, SLOT, TIMESTAMP - 1, INTERVAL),
            err(MarketTickError::NonMonotonic)
        );
        assert_eq!(state, before);

        increment(&mut state, signer(1), SLOT, SLOT, TIMESTAMP + 1, INTERVAL).unwrap();
        assert_eq!(state.timestamp_ns, TIMESTAMP + 1);
        assert_eq!(state.first_timestamp_ns, TIMESTAMP);
        assert_eq!(state.sequence, 1);
        assert_eq!(state.observed_market_tick_interval_ns, 1);
    }

    #[test]
    fn observed_interval_covers_the_full_positive_range() {
        let mut state = initialized();
        increment(&mut state, signer(1), SLOT, SLOT, 1, INTERVAL).unwrap();
        increment(&mut state, signer(1), SLOT, SLOT, i64::MAX, INTERVAL).unwrap();
        assert_eq!(
            state.observed_market_tick_interval_ns,
            (i64::MAX - 1) as u64
        );
        assert_eq!(state.sequence, 1);
    }

    #[test]
    fn same_slot_rejects_foreign_signer_and_interval_change() {
        let mut state = initialized();
        increment(&mut state, signer(1), SLOT, SLOT, TIMESTAMP, INTERVAL).unwrap();
        let before = state;

        assert_eq!(
            increment(&mut state, signer(2), SLOT, SLOT, TIMESTAMP + 1, INTERVAL),
            err(MarketTickError::NotSlotSigner)
        );
        assert_eq!(state, before);
        assert_eq!(
            increment(
                &mut state,
                signer(1),
                SLOT,
                SLOT,
                TIMESTAMP + 1,
                INTERVAL + 1
            ),
            err(MarketTickError::IntervalMismatch)
        );
        assert_eq!(state, before);
    }

    #[test]
    fn error_precedence_matches_original_program() {
        let mut state = initialized();
        increment(&mut state, signer(1), SLOT, SLOT, TIMESTAMP, INTERVAL).unwrap();
        let before = state;

        assert_eq!(
            increment(&mut state, signer(2), SLOT, SLOT + 1, 0, 0),
            err(MarketTickError::SlotMismatch)
        );
        assert_eq!(
            increment(&mut state, signer(2), SLOT, SLOT, 0, 0),
            err(MarketTickError::InvalidTimestamp)
        );
        assert_eq!(
            increment(&mut state, signer(2), SLOT, SLOT, 1, 0),
            err(MarketTickError::InvalidInterval)
        );
        assert_eq!(
            increment(&mut state, signer(2), SLOT, SLOT, 1, INTERVAL + 1),
            err(MarketTickError::NotSlotSigner)
        );
        assert_eq!(
            increment(&mut state, signer(1), SLOT, SLOT, 1, INTERVAL + 1),
            err(MarketTickError::IntervalMismatch)
        );
        assert_eq!(state, before);
    }

    #[test]
    fn invalid_arguments_are_rejected_without_mutation() {
        let mut state = initialized();
        for (timestamp_ns, interval_ns, expected) in [
            (0, INTERVAL, MarketTickError::InvalidTimestamp),
            (-1, INTERVAL, MarketTickError::InvalidTimestamp),
            (i64::MIN, INTERVAL, MarketTickError::InvalidTimestamp),
            (TIMESTAMP, 0, MarketTickError::InvalidInterval),
        ] {
            assert_eq!(
                increment(&mut state, signer(1), SLOT, SLOT, timestamp_ns, interval_ns),
                err(expected)
            );
            assert_eq!(state, initialized());
        }
    }

    #[test]
    fn sequence_overflow_is_rejected_without_mutating_state() {
        let mut state = MarketTickV1State {
            signer: signer(1),
            slot: SLOT,
            first_timestamp_ns: TIMESTAMP,
            timestamp_ns: TIMESTAMP,
            sequence: u64::MAX,
            target_market_tick_interval_ns: INTERVAL,
            ..initialized()
        };
        let before = state;

        assert_eq!(
            increment(&mut state, signer(1), SLOT, SLOT, TIMESTAMP + 1, INTERVAL),
            err(MarketTickError::CounterOverflow)
        );
        assert_eq!(state, before);
    }

    #[test]
    fn instruction_slot_must_match_current_slot() {
        let mut state = initialized();
        assert_eq!(
            increment(&mut state, signer(1), SLOT, SLOT - 1, TIMESTAMP, INTERVAL),
            err(MarketTickError::SlotMismatch)
        );
        assert_eq!(state, initialized());
    }
}
