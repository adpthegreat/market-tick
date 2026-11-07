use core::ptr::read_unaligned;

use pinocchio::program_error::ProgramError;

// ### Instruction data:
//   [0]       tag = 1                          u8
//   [1..9]    slot                             u64 le
//   [9..17]   timestamp_ns                     i64 le
//   [17..25]  target_market_tick_interval_ns   u64 le
//   [25..]    ignored

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IncrementData {
    pub slot: u64,
    pub timestamp_ns: i64,
    pub target_market_tick_interval_ns: u64,
}

impl IncrementData {
    /// Serialized length including the tag byte.
    pub const LEN: usize = 25;

    /// Decodes the fields of a tagged increment instruction.
    #[inline(always)]
    pub fn try_from_bytes(data: &[u8]) -> Result<Self, ProgramError> {
        if data.len() < Self::LEN {
            return Err(ProgramError::InvalidInstructionData);
        }
        let ptr = data.as_ptr();
        // SAFETY: `data` holds at least 25 bytes, so every 8-byte read at
        // offsets 1, 9 and 17 is in bounds. The reads are unaligned because the
        // fields start one byte after the tag.
        unsafe {
            Ok(Self {
                slot: read_unaligned(ptr.add(1) as *const u64),
                timestamp_ns: read_unaligned(ptr.add(9) as *const i64),
                target_market_tick_interval_ns: read_unaligned(ptr.add(17) as *const u64),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    fn encode(slot: u64, timestamp_ns: i64, interval_ns: u64) -> Vec<u8> {
        let mut bytes = std::vec![1u8];
        bytes.extend_from_slice(&slot.to_le_bytes());
        bytes.extend_from_slice(&timestamp_ns.to_le_bytes());
        bytes.extend_from_slice(&interval_ns.to_le_bytes());
        bytes
    }

    #[test]
    fn decodes_little_endian_fields() {
        let slot = 0x0102_0304_0506_0708;
        let timestamp_ns = -0x0102_0304_0506_0708;
        let interval_ns = 0x1112_1314_1516_1718;
        let bytes = encode(slot, timestamp_ns, interval_ns);
        assert_eq!(bytes.len(), IncrementData::LEN);
        assert_eq!(
            IncrementData::try_from_bytes(&bytes).unwrap(),
            IncrementData {
                slot,
                timestamp_ns,
                target_market_tick_interval_ns: interval_ns,
            }
        );
    }

    #[test]
    fn rejects_every_short_length() {
        let bytes = encode(1, 2, 3);
        for len in 0..IncrementData::LEN {
            assert_eq!(
                IncrementData::try_from_bytes(&bytes[..len]),
                Err(ProgramError::InvalidInstructionData)
            );
        }
    }

    #[test]
    fn ignores_trailing_bytes() {
        let mut bytes = encode(7, 8, 9);
        bytes.extend_from_slice(&[0xFF; 64]);
        assert_eq!(
            IncrementData::try_from_bytes(&bytes).unwrap(),
            IncrementData {
                slot: 7,
                timestamp_ns: 8,
                target_market_tick_interval_ns: 9,
            }
        );
    }

    #[test]
    fn decodes_at_every_buffer_alignment() {
        let bytes = encode(u64::MAX, i64::MIN, 0x8000_0000_0000_0001);
        let mut backing = std::vec![0u8; IncrementData::LEN + 16];
        for offset in 0..16 {
            backing[offset..offset + IncrementData::LEN].copy_from_slice(&bytes);
            let view = &backing[offset..offset + IncrementData::LEN];
            assert_eq!(
                IncrementData::try_from_bytes(view).unwrap(),
                IncrementData {
                    slot: u64::MAX,
                    timestamp_ns: i64::MIN,
                    target_market_tick_interval_ns: 0x8000_0000_0000_0001,
                }
            );
        }
    }
}
