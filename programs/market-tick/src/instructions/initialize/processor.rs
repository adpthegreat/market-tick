use market_tick_abi::{PDA_SEED_MARKET_TICK, PDA_SEED_VERSION_V1};
use pinocchio::{
    instruction::{Seed, Signer},
    sysvars::{rent::Rent, Sysvar},
    ProgramResult,
};
use pinocchio_system::instructions::{Allocate, Assign, CreateAccount, Transfer};

use super::InitializeAccounts;
use crate::{
    state::{MarketTickV1State, DISCRIMINATOR_V1_U64},
    PDA_V1_BUMP,
};

const SPACE: u64 = MarketTickV1State::LEN as u64;

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
    let signer = [Signer::from(&seeds)];

    let lamports = pda.lamports();
    if lamports == 0 {
        CreateAccount {
            from: payer,
            to: pda,
            lamports: rent_exempt,
            space: SPACE,
            owner: &crate::ID,
        }
        .invoke_signed(&signer)?;
    } else {
        let shortfall = rent_exempt.saturating_sub(lamports);
        if shortfall > 0 {
            Transfer {
                from: payer,
                to: pda,
                lamports: shortfall,
            }
            .invoke()?;
        }
        Allocate {
            account: pda,
            space: SPACE,
        }
        .invoke_signed(&signer)?;
        Assign {
            account: pda,
            owner: &crate::ID,
        }
        .invoke_signed(&signer)?;
    }

    // SAFETY: the system program just sized the account to `SPACE` bytes and
    // the runtime reflected that in the serialized input, whose data regions
    // are 8-byte aligned. No other reference to the data exists.
    unsafe { (pda.data_ptr() as *mut u64).write(DISCRIMINATOR_V1_U64) };
    Ok(())
}
