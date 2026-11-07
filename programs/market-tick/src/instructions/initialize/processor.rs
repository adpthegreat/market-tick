use market_tick_abi::{PDA_SEED_MARKET_TICK, PDA_SEED_VERSION_V1};
use pinocchio::{
    cpi::invoke_signed_unchecked,
    instruction::{Account, AccountMeta, Instruction, Seed, Signer},
    sysvars::{rent::Rent, Sysvar},
    ProgramResult,
};

use super::InitializeAccounts;
use crate::{
    state::{MarketTickV1State, DISCRIMINATOR_V1_U64},
    PDA_V1_BUMP,
};

const SPACE: u64 = MarketTickV1State::LEN as u64;

const SYSTEM_CREATE_ACCOUNT: u8 = 0;
const SYSTEM_ASSIGN: u8 = 1;
const SYSTEM_TRANSFER: u8 = 2;
const SYSTEM_ALLOCATE: u8 = 8;

#[inline(never)]
fn invoke_system(data: &[u8], metas: &[AccountMeta], accounts: &[Account], signers: &[Signer]) {
    let instruction = Instruction {
        program_id: &crate::SYSTEM_PROGRAM_ID,
        accounts: metas,
        data,
    };
    // SAFETY: no borrow of any account's lamports or data is alive across the
    // call. A failing system instruction aborts this program in the runtime.
    unsafe { invoke_signed_unchecked(&instruction, accounts, signers) }
}

#[inline(always)]
fn system_data<const N: usize>(tag: u8, words: &[u64], tail: &[u8]) -> [u8; N] {
    let mut data = [0u8; N];
    data[0] = tag;
    let mut offset = 4;
    for word in words {
        data[offset..offset + 8].copy_from_slice(&word.to_le_bytes());
        offset += 8;
    }
    data[offset..offset + tail.len()].copy_from_slice(tail);
    data
}

/// Creates the canonical V1 PDA, including when it was prefunded, and writes
/// the discriminator. All state fields stay zero.
#[inline(always)]
pub fn process_initialize(accounts: &InitializeAccounts) -> ProgramResult {
    let InitializeAccounts { payer, pda, .. } = accounts;

    let rent_exempt = Rent::get()?.minimum_balance(MarketTickV1State::LEN);

    let bump = [PDA_V1_BUMP];
    let seeds = [
        Seed::from(PDA_SEED_MARKET_TICK),
        Seed::from(PDA_SEED_VERSION_V1),
        Seed::from(&bump),
    ];
    let pda_signer = [Signer::from(&seeds)];
    let payer_and_pda = [Account::from(payer), Account::from(pda)];
    let pda_only = [Account::from(pda)];

    let lamports = pda.lamports();
    if lamports == 0 {
        invoke_system(
            &system_data::<52>(SYSTEM_CREATE_ACCOUNT, &[rent_exempt, SPACE], &crate::ID),
            &[
                AccountMeta::writable_signer(payer.key()),
                AccountMeta::writable_signer(pda.key()),
            ],
            &payer_and_pda,
            &pda_signer,
        );
    } else {
        let shortfall = rent_exempt.saturating_sub(lamports);
        if shortfall > 0 {
            invoke_system(
                &system_data::<12>(SYSTEM_TRANSFER, &[shortfall], &[]),
                &[
                    AccountMeta::writable_signer(payer.key()),
                    AccountMeta::writable(pda.key()),
                ],
                &payer_and_pda,
                &[],
            );
        }
        let pda_meta = [AccountMeta::writable_signer(pda.key())];
        invoke_system(
            &system_data::<12>(SYSTEM_ALLOCATE, &[SPACE], &[]),
            &pda_meta,
            &pda_only,
            &pda_signer,
        );
        invoke_system(
            &system_data::<36>(SYSTEM_ASSIGN, &[], &crate::ID),
            &pda_meta,
            &pda_only,
            &pda_signer,
        );
    }

    // SAFETY: the system program just sized the account to `SPACE` bytes and
    // the runtime reflected that in the serialized input, whose data regions
    // are 8-byte aligned. No other reference to the data exists.
    unsafe { (pda.data_ptr() as *mut u64).write(DISCRIMINATOR_V1_U64) };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_instruction_encodings_match_the_system_program() {
        let owner = [7u8; 32];

        let create = system_data::<52>(SYSTEM_CREATE_ACCOUNT, &[0x0102, 88], &owner);
        let mut expected = std::vec![0u8, 0, 0, 0];
        expected.extend_from_slice(&0x0102u64.to_le_bytes());
        expected.extend_from_slice(&88u64.to_le_bytes());
        expected.extend_from_slice(&owner);
        assert_eq!(&create[..], &expected[..]);

        let transfer = system_data::<12>(SYSTEM_TRANSFER, &[u64::MAX - 1], &[]);
        let mut expected = std::vec![2u8, 0, 0, 0];
        expected.extend_from_slice(&(u64::MAX - 1).to_le_bytes());
        assert_eq!(&transfer[..], &expected[..]);

        let allocate = system_data::<12>(SYSTEM_ALLOCATE, &[88], &[]);
        let mut expected = std::vec![8u8, 0, 0, 0];
        expected.extend_from_slice(&88u64.to_le_bytes());
        assert_eq!(&allocate[..], &expected[..]);

        let assign = system_data::<36>(SYSTEM_ASSIGN, &[], &owner);
        let mut expected = std::vec![1u8, 0, 0, 0];
        expected.extend_from_slice(&owner);
        assert_eq!(&assign[..], &expected[..]);
    }
}
