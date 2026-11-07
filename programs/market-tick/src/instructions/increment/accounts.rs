use pinocchio::{
    account_info::AccountInfo, entrypoint::MaybeAccount, program_error::ProgramError,
    pubkey::pubkey_eq,
};

use crate::{state::MarketTickV1State, PDA_V1};

// ### Accounts:
//   0. `[WRITE]`   pda      canonical V1 PDA, program-owned, initialized
//   1. `[SIGNER]`  signer
//   +N             ignored

pub struct IncrementAccounts {
    pub pda: AccountInfo,
    pub signer: AccountInfo,
}

impl IncrementAccounts {
    #[inline(always)]
    pub fn try_from_accounts(
        pda: MaybeAccount,
        signer: MaybeAccount,
    ) -> Result<Self, ProgramError> {
        let (MaybeAccount::Account(pda), MaybeAccount::Account(signer)) = (pda, signer) else {
            return Err(ProgramError::InvalidArgument);
        };

        if !pubkey_eq(pda.owner(), &crate::ID) {
            return Err(ProgramError::InvalidAccountOwner);
        }
        if pda.data_len() != MarketTickV1State::LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        // SAFETY: the data region holds exactly `LEN` bytes and starts 8-byte
        // aligned in the runtime input; the discriminator is its first u64.
        if unsafe { *(pda.data_ptr() as *const u64) } != crate::state::DISCRIMINATOR_V1_U64 {
            return Err(ProgramError::InvalidAccountData);
        }
        if !pda.is_writable() {
            return Err(ProgramError::Immutable);
        }
        if !signer.is_signer() {
            return Err(ProgramError::MissingRequiredSignature);
        }
        if !pubkey_eq(pda.key(), &PDA_V1) {
            return Err(ProgramError::InvalidSeeds);
        }

        Ok(Self { pda, signer })
    }
}
