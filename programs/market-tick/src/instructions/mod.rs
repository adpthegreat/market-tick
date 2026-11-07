pub mod increment;
pub mod initialize;

pub use increment::{increment_v1, process_increment, IncrementAccounts, IncrementData};
pub use initialize::{process_initialize, InitializeAccounts};

use pinocchio::{entrypoint::InstructionContext, program_error::ProgramError};

pub const INITIALIZE_TAG: u8 = 0;
pub const INCREMENT_TAG: u8 = 1;

pub enum MarketTickInstruction {
    Initialize(InitializeAccounts),
    Increment(IncrementAccounts, IncrementData),
}

/// Walks the serialized input once and decodes the instruction.
///
/// Accounts past the ones an instruction uses and trailing instruction
/// bytes are accepted and ignored. Instruction data is validated before
/// accounts, matching the original program's error precedence.
#[inline(always)]
pub fn parse(mut context: InstructionContext) -> Result<MarketTickInstruction, ProgramError> {
    let count = context.remaining();
    if count < 2 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }

    // SAFETY: `count` accounts are serialized in the input; exactly `count`
    // accounts are consumed below, after which the cursor points at the
    // instruction data length.
    let (first, second, third, data) = unsafe {
        let first = context.next_account_unchecked();
        let second = context.next_account_unchecked();
        let third = if count > 2 {
            Some(context.next_account_unchecked())
        } else {
            None
        };
        let mut index = 3;
        while index < count {
            context.next_account_unchecked();
            index += 1;
        }
        (first, second, third, context.instruction_data_unchecked())
    };

    match data.first() {
        Some(&INITIALIZE_TAG) => Ok(MarketTickInstruction::Initialize(
            InitializeAccounts::try_from_accounts(first, second, third)?,
        )),
        Some(&INCREMENT_TAG) => {
            let data = IncrementData::try_from_bytes(data)?;
            let accounts = IncrementAccounts::try_from_accounts(first, second)?;
            Ok(MarketTickInstruction::Increment(accounts, data))
        }
        _ => Err(ProgramError::InvalidInstructionData),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        state::{MarketTickV1State, DISCRIMINATOR_V1_U64},
        test_utils::{InputAccount, SerializedInput, TestAccount},
        PDA_V1,
    };
    use market_tick_abi::{MarketTickV1, DISCRIMINATOR_V1};
    use pinocchio::{pubkey::Pubkey, ProgramResult};
    use std::{vec, vec::Vec};

    const SYSTEM: Pubkey = pinocchio_system::ID;
    const SIGNER: Pubkey = [0x22; 32];
    const PAYER: Pubkey = [0x33; 32];

    fn parse_input(input: &mut SerializedInput) -> Result<MarketTickInstruction, ProgramError> {
        // SAFETY: `SerializedInput` reproduces the runtime input layout.
        parse(unsafe { InstructionContext::new_unchecked(input.as_mut_ptr()) })
    }

    fn initialized_data() -> Vec<u8> {
        let mut data = vec![0u8; MarketTickV1::LEN];
        data[..8].copy_from_slice(&DISCRIMINATOR_V1);
        data
    }

    fn tick_account() -> TestAccount {
        TestAccount::new(PDA_V1, crate::ID, initialized_data()).writable()
    }

    fn signer_account() -> TestAccount {
        TestAccount::new(SIGNER, SYSTEM, vec![]).signer()
    }

    fn increment_data(slot: u64, timestamp_ns: i64, interval_ns: u64) -> Vec<u8> {
        let mut data = vec![INCREMENT_TAG];
        data.extend_from_slice(&slot.to_le_bytes());
        data.extend_from_slice(&timestamp_ns.to_le_bytes());
        data.extend_from_slice(&interval_ns.to_le_bytes());
        data
    }

    fn increment_input(accounts: Vec<InputAccount>, data: &[u8]) -> SerializedInput {
        SerializedInput::new(&accounts, data, &crate::ID)
    }

    fn standard_increment_accounts() -> Vec<InputAccount> {
        vec![
            InputAccount::Account(tick_account()),
            InputAccount::Account(signer_account()),
        ]
    }

    fn initialize_accounts() -> Vec<InputAccount> {
        vec![
            InputAccount::Account(TestAccount::new(PAYER, SYSTEM, vec![]).signer().writable()),
            InputAccount::Account(
                TestAccount::new(PDA_V1, SYSTEM, vec![])
                    .writable()
                    .lamports(0),
            ),
            InputAccount::Account(TestAccount::new(SYSTEM, [0xAA; 32], vec![])),
        ]
    }

    fn expect_err(input: &mut SerializedInput, expected: ProgramError) {
        match parse_input(input) {
            Err(error) => assert_eq!(error, expected),
            Ok(_) => panic!("expected {expected:?}"),
        }
    }

    fn apply_increment(input: &mut SerializedInput, current_slot: u64) -> ProgramResult {
        let MarketTickInstruction::Increment(accounts, data) = parse_input(input)? else {
            panic!("expected increment");
        };
        // SAFETY: parsing validated length; the builder aligns account data.
        let state = unsafe { MarketTickV1State::from_account_mut_unchecked(&accounts.pda) };
        increment_v1(state, accounts.signer.key(), current_slot, data)
    }

    #[test]
    fn increment_parses_accounts_and_data() {
        let mut input = increment_input(standard_increment_accounts(), &increment_data(5, 6, 7));
        let MarketTickInstruction::Increment(accounts, data) = parse_input(&mut input).unwrap()
        else {
            panic!("expected increment");
        };
        assert_eq!(accounts.pda.key(), &PDA_V1);
        assert_eq!(accounts.signer.key(), &SIGNER);
        assert_eq!(
            data,
            IncrementData {
                slot: 5,
                timestamp_ns: 6,
                target_market_tick_interval_ns: 7,
            }
        );
    }

    #[test]
    fn increment_writes_abi_encoded_state_in_place() {
        let mut input = increment_input(standard_increment_accounts(), &increment_data(9, 100, 10));
        apply_increment(&mut input, 9).unwrap();
        let mut input = {
            let data = input.account_data(0, MarketTickV1::LEN).to_vec();
            let mut tick = tick_account();
            tick.data = data;
            increment_input(
                vec![
                    InputAccount::Account(tick),
                    InputAccount::Account(signer_account()),
                ],
                &increment_data(9, 160, 10),
            )
        };
        apply_increment(&mut input, 9).unwrap();

        let decoded = MarketTickV1::decode(input.account_data(0, MarketTickV1::LEN)).unwrap();
        assert_eq!(
            decoded,
            MarketTickV1 {
                signer: SIGNER,
                slot: 9,
                first_timestamp_ns: 100,
                timestamp_ns: 160,
                sequence: 1,
                target_market_tick_interval_ns: 10,
                observed_market_tick_interval_ns: 60,
            }
        );
    }

    #[test]
    fn rejected_increment_leaves_account_bytes_untouched() {
        let mut input = increment_input(standard_increment_accounts(), &increment_data(9, 0, 10));
        let before = input.account_data(0, MarketTickV1::LEN).to_vec();
        assert!(apply_increment(&mut input, 9).is_err());
        assert_eq!(input.account_data(0, MarketTickV1::LEN), &before[..]);
    }

    #[test]
    fn trailing_accounts_and_data_are_skipped() {
        let mut accounts = standard_increment_accounts();
        for len in [0usize, 1, 7, 8, 9, 31, 88, 333] {
            accounts.push(InputAccount::Account(TestAccount::new(
                [len as u8; 32],
                SYSTEM,
                vec![0xEE; len],
            )));
            accounts.push(InputAccount::Duplicate(1));
        }
        let mut data = increment_data(1, 2, 3);
        data.extend_from_slice(&[0xFF; 13]);
        let mut input = increment_input(accounts, &data);
        let MarketTickInstruction::Increment(_, data) = parse_input(&mut input).unwrap() else {
            panic!("expected increment");
        };
        assert_eq!(data.slot, 1);
        assert_eq!(data.timestamp_ns, 2);
        assert_eq!(data.target_market_tick_interval_ns, 3);
    }

    #[test]
    fn rejects_too_few_accounts() {
        for count in 0..2 {
            let accounts: Vec<_> = standard_increment_accounts()
                .into_iter()
                .take(count)
                .collect();
            expect_err(
                &mut increment_input(accounts, &increment_data(1, 2, 3)),
                ProgramError::NotEnoughAccountKeys,
            );
        }
    }

    #[test]
    fn rejects_unknown_and_missing_tags() {
        for data in [&[][..], &[2u8][..], &[0xFF; 25][..]] {
            expect_err(
                &mut increment_input(standard_increment_accounts(), data),
                ProgramError::InvalidInstructionData,
            );
        }
    }

    #[test]
    fn rejects_short_increment_data_before_checking_accounts() {
        let full = increment_data(1, 2, 3);
        for len in 1..IncrementData::LEN {
            expect_err(
                &mut increment_input(
                    vec![
                        InputAccount::Account(TestAccount::new([0; 32], [0; 32], vec![])),
                        InputAccount::Duplicate(0),
                    ],
                    &full[..len],
                ),
                ProgramError::InvalidInstructionData,
            );
        }
    }

    #[test]
    fn rejects_duplicate_increment_accounts() {
        expect_err(
            &mut increment_input(
                vec![
                    InputAccount::Account(tick_account()),
                    InputAccount::Duplicate(0),
                ],
                &increment_data(1, 2, 3),
            ),
            ProgramError::InvalidArgument,
        );
    }

    #[test]
    fn rejects_invalid_tick_accounts() {
        let cases: Vec<(TestAccount, ProgramError)> = vec![
            (
                TestAccount::new(PDA_V1, SYSTEM, initialized_data()).writable(),
                ProgramError::InvalidAccountOwner,
            ),
            (
                TestAccount::new(PDA_V1, crate::ID, vec![]).writable(),
                ProgramError::InvalidAccountData,
            ),
            (
                TestAccount::new(PDA_V1, crate::ID, initialized_data()[..87].to_vec()).writable(),
                ProgramError::InvalidAccountData,
            ),
            (
                TestAccount::new(PDA_V1, crate::ID, {
                    let mut data = initialized_data();
                    data.push(0);
                    data
                })
                .writable(),
                ProgramError::InvalidAccountData,
            ),
            (
                TestAccount::new(PDA_V1, crate::ID, vec![0; MarketTickV1::LEN]).writable(),
                ProgramError::InvalidAccountData,
            ),
            (
                TestAccount::new(PDA_V1, crate::ID, initialized_data()),
                ProgramError::Immutable,
            ),
            (
                TestAccount::new([0x44; 32], crate::ID, initialized_data()).writable(),
                ProgramError::InvalidSeeds,
            ),
        ];
        for (tick, expected) in cases {
            expect_err(
                &mut increment_input(
                    vec![
                        InputAccount::Account(tick),
                        InputAccount::Account(signer_account()),
                    ],
                    &increment_data(1, 2, 3),
                ),
                expected,
            );
        }
    }

    #[test]
    fn rejects_unsigned_increment() {
        expect_err(
            &mut increment_input(
                vec![
                    InputAccount::Account(tick_account()),
                    InputAccount::Account(TestAccount::new(SIGNER, SYSTEM, vec![])),
                ],
                &increment_data(1, 2, 3),
            ),
            ProgramError::MissingRequiredSignature,
        );
    }

    #[test]
    fn initialize_parses_with_trailing_accounts_and_data() {
        let mut accounts = initialize_accounts();
        accounts.push(InputAccount::Duplicate(0));
        accounts.push(InputAccount::Account(TestAccount::new(
            [9; 32],
            SYSTEM,
            vec![1, 2, 3],
        )));
        let mut input = SerializedInput::new(&accounts, &[INITIALIZE_TAG, 0, 0], &crate::ID);
        let MarketTickInstruction::Initialize(accounts) = parse_input(&mut input).unwrap() else {
            panic!("expected initialize");
        };
        assert_eq!(accounts.payer.key(), &PAYER);
        assert_eq!(accounts.pda.key(), &PDA_V1);
        assert_eq!(accounts.system_program.key(), &SYSTEM);
    }

    #[test]
    fn initialize_rejects_invalid_accounts() {
        let mutate = |index: usize, f: &dyn Fn(&mut TestAccount)| {
            let mut accounts = initialize_accounts();
            if let InputAccount::Account(account) = &mut accounts[index] {
                f(account);
            }
            accounts
        };
        let cases: Vec<(Vec<InputAccount>, ProgramError)> = vec![
            (
                initialize_accounts().into_iter().take(2).collect(),
                ProgramError::NotEnoughAccountKeys,
            ),
            (
                {
                    let mut accounts = initialize_accounts();
                    accounts[1] = InputAccount::Duplicate(0);
                    accounts
                },
                ProgramError::InvalidArgument,
            ),
            (
                {
                    let mut accounts = initialize_accounts();
                    accounts[2] = InputAccount::Duplicate(1);
                    accounts
                },
                ProgramError::InvalidArgument,
            ),
            (
                mutate(0, &|a| a.is_signer = false),
                ProgramError::MissingRequiredSignature,
            ),
            (
                mutate(0, &|a| a.is_writable = false),
                ProgramError::Immutable,
            ),
            (
                mutate(1, &|a| a.is_writable = false),
                ProgramError::Immutable,
            ),
            (
                mutate(1, &|a| a.key = [0x55; 32]),
                ProgramError::InvalidSeeds,
            ),
            (
                mutate(1, &|a| a.owner = crate::ID),
                ProgramError::AccountAlreadyInitialized,
            ),
            (
                mutate(2, &|a| a.key = [0x66; 32]),
                ProgramError::IncorrectProgramId,
            ),
        ];
        for (accounts, expected) in cases {
            expect_err(
                &mut SerializedInput::new(&accounts, &[INITIALIZE_TAG], &crate::ID),
                expected,
            );
        }
    }

    #[test]
    fn randomized_inputs_parse_without_ub() {
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let iterations = if cfg!(miri) { 24 } else { 2_000 };

        for _ in 0..iterations {
            let account_count = (next() % 6) as usize;
            let mut accounts = Vec::with_capacity(account_count);
            for index in 0..account_count {
                let roll = next();
                if index > 0 && roll % 4 == 0 {
                    accounts.push(InputAccount::Duplicate((roll % index as u64) as u8));
                    continue;
                }
                let data_len = match roll % 5 {
                    0 => MarketTickV1::LEN,
                    _ => (roll >> 8) as usize % 130,
                };
                let mut data: Vec<u8> = (0..data_len).map(|_| next() as u8).collect();
                if roll % 3 == 0 && data_len >= 8 {
                    data[..8].copy_from_slice(&DISCRIMINATOR_V1);
                }
                let mut account = TestAccount::new(
                    if roll % 2 == 0 {
                        PDA_V1
                    } else {
                        [next() as u8; 32]
                    },
                    match roll % 3 {
                        0 => crate::ID,
                        1 => SYSTEM,
                        _ => [next() as u8; 32],
                    },
                    data,
                );
                account.is_signer = next() % 2 == 0;
                account.is_writable = next() % 2 == 0;
                accounts.push(InputAccount::Account(account));
            }
            let data_len = (next() % 40) as usize;
            let mut data: Vec<u8> = (0..data_len).map(|_| next() as u8).collect();
            if let Some(tag) = data.first_mut() {
                *tag %= 3;
            }

            let mut input = SerializedInput::new(&accounts, &data, &crate::ID);
            if let Ok(MarketTickInstruction::Increment(..)) = parse_input(&mut input) {
                let before = input.account_data(0, MarketTickV1::LEN).to_vec();
                let slot = IncrementData::try_from_bytes(&data).unwrap().slot;
                let result = apply_increment(&mut input, slot);
                let after = input.account_data(0, MarketTickV1::LEN);
                assert_eq!(&after[..8], &DISCRIMINATOR_V1);
                if result.is_err() {
                    assert_eq!(after, &before[..]);
                }
                assert_eq!(
                    MarketTickV1State::read(after).unwrap().discriminator,
                    DISCRIMINATOR_V1_U64
                );
            }
        }
    }
}
