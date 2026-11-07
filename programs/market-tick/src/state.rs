use core::mem::{align_of, offset_of, size_of};

use market_tick_abi::{MarketTickV1, DISCRIMINATOR_V1};
use pinocchio::{account_info::AccountInfo, pubkey::Pubkey};

/// In-place view of the 88-byte V1 account.
///
/// Field order and widths are identical to the encoded ABI, so on a
/// little-endian target the account bytes can be reinterpreted directly.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MarketTickV1State {
    pub discriminator: u64,
    pub signer: Pubkey,
    pub slot: u64,
    pub first_timestamp_ns: i64,
    pub timestamp_ns: i64,
    pub sequence: u64,
    pub target_market_tick_interval_ns: u64,
    pub observed_market_tick_interval_ns: u64,
}

pub const DISCRIMINATOR_V1_U64: u64 = u64::from_le_bytes(DISCRIMINATOR_V1);

const _: () = {
    assert!(size_of::<MarketTickV1State>() == MarketTickV1::LEN);
    assert!(align_of::<MarketTickV1State>() == 8);
    assert!(offset_of!(MarketTickV1State, discriminator) == 0);
    assert!(offset_of!(MarketTickV1State, signer) == 8);
    assert!(offset_of!(MarketTickV1State, slot) == 40);
    assert!(offset_of!(MarketTickV1State, first_timestamp_ns) == 48);
    assert!(offset_of!(MarketTickV1State, timestamp_ns) == 56);
    assert!(offset_of!(MarketTickV1State, sequence) == 64);
    assert!(offset_of!(MarketTickV1State, target_market_tick_interval_ns) == 72);
    assert!(offset_of!(MarketTickV1State, observed_market_tick_interval_ns) == 80);
};

impl MarketTickV1State {
    pub const LEN: usize = MarketTickV1::LEN;

    /// Reinterprets the account's data region as a V1 state.
    ///
    /// # Safety
    ///
    /// * `account.data_len()` must be at least [`Self::LEN`].
    /// * The data pointer must be 8-byte aligned. The runtime input
    ///   serialization guarantees this for every account.
    /// * No other reference to the account data may be alive for the
    ///   returned lifetime.
    #[inline(always)]
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn from_account_mut_unchecked(account: &AccountInfo) -> &mut Self {
        debug_assert!(account.data_len() >= Self::LEN);
        debug_assert!((account.data_ptr() as usize).is_multiple_of(align_of::<Self>()));
        &mut *(account.data_ptr() as *mut Self)
    }

    /// Reads a V1 state from an arbitrary (possibly unaligned) byte slice.
    #[inline]
    pub fn read(data: &[u8]) -> Option<Self> {
        if data.len() != Self::LEN {
            return None;
        }
        // SAFETY: length checked above; every bit pattern is a valid value of
        // this plain-integer struct; `read_unaligned` imposes no alignment.
        Some(unsafe { core::ptr::read_unaligned(data.as_ptr() as *const Self) })
    }

    #[inline(always)]
    pub fn has_valid_discriminator(&self) -> bool {
        self.discriminator == DISCRIMINATOR_V1_U64
    }
}

impl From<MarketTickV1State> for MarketTickV1 {
    fn from(state: MarketTickV1State) -> Self {
        Self {
            signer: state.signer,
            slot: state.slot,
            first_timestamp_ns: state.first_timestamp_ns,
            timestamp_ns: state.timestamp_ns,
            sequence: state.sequence,
            target_market_tick_interval_ns: state.target_market_tick_interval_ns,
            observed_market_tick_interval_ns: state.observed_market_tick_interval_ns,
        }
    }
}

impl From<MarketTickV1> for MarketTickV1State {
    fn from(account: MarketTickV1) -> Self {
        Self {
            discriminator: DISCRIMINATOR_V1_U64,
            signer: account.signer,
            slot: account.slot,
            first_timestamp_ns: account.first_timestamp_ns,
            timestamp_ns: account.timestamp_ns,
            sequence: account.sequence,
            target_market_tick_interval_ns: account.target_market_tick_interval_ns,
            observed_market_tick_interval_ns: account.observed_market_tick_interval_ns,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> MarketTickV1 {
        MarketTickV1 {
            signer: core::array::from_fn(|i| i as u8 + 1),
            slot: 0x0102_0304_0506_0708,
            first_timestamp_ns: -0x1112_1314_1516_1718,
            timestamp_ns: i64::MAX,
            sequence: u64::MAX - 1,
            target_market_tick_interval_ns: 0x2122_2324_2526_2728,
            observed_market_tick_interval_ns: 0x3132_3334_3536_3738,
        }
    }

    #[test]
    fn in_place_layout_matches_abi_encoding() {
        let expected = sample();
        let mut encoded = [0u8; MarketTickV1::LEN];
        expected.encode(&mut encoded).unwrap();

        let state = MarketTickV1State::read(&encoded).unwrap();
        assert!(state.has_valid_discriminator());
        assert_eq!(MarketTickV1::from(state), expected);
        assert_eq!(MarketTickV1State::from(expected), state);
    }

    #[test]
    fn read_accepts_every_unaligned_offset() {
        let expected = sample();
        let mut backing = [0u8; MarketTickV1::LEN + 16];
        for offset in 0..16 {
            backing.fill(0xAA);
            expected
                .encode(&mut backing[offset..offset + MarketTickV1::LEN])
                .unwrap();
            let state =
                MarketTickV1State::read(&backing[offset..offset + MarketTickV1::LEN]).unwrap();
            assert_eq!(MarketTickV1::from(state), expected);
        }
    }

    #[test]
    fn read_rejects_wrong_lengths() {
        let backing = [0u8; MarketTickV1::LEN + 1];
        for len in 0..backing.len() {
            assert_eq!(
                MarketTickV1State::read(&backing[..len]).is_some(),
                len == MarketTickV1::LEN
            );
        }
    }

    #[test]
    fn discriminator_constant_matches_abi_bytes() {
        assert_eq!(DISCRIMINATOR_V1_U64.to_le_bytes(), DISCRIMINATOR_V1);
        let mut bad = MarketTickV1State::from(sample());
        bad.discriminator ^= 1;
        assert!(!bad.has_valid_discriminator());
    }
}
