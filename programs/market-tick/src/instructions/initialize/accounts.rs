use pinocchio::{
    account_info::AccountInfo, entrypoint::MaybeAccount, program_error::ProgramError,
    pubkey::pubkey_eq,
};

use crate::PDA_V1;

// ### Accounts:
//   0. `[WRITE, SIGNER]`  payer
//   1. `[WRITE]`          pda             canonical V1 PDA, system-owned
//   2. `[]`               system_program
//   +N                    ignored

pub struct InitializeAccounts {
    pub payer: AccountInfo,
    pub pda: AccountInfo,
    pub system_program: AccountInfo,
}

impl InitializeAccounts {
    #[inline(always)]
    pub fn try_from_accounts(
        payer: MaybeAccount,
        pda: MaybeAccount,
        system_program: Option<MaybeAccount>,
    ) -> Result<Self, ProgramError> {
        let Some(system_program) = system_program else {
            return Err(ProgramError::NotEnoughAccountKeys);
        };
        let (
            MaybeAccount::Account(payer),
            MaybeAccount::Account(pda),
            MaybeAccount::Account(system_program),
        ) = (payer, pda, system_program)
        else {
            return Err(ProgramError::InvalidArgument);
        };

        if !payer.is_signer() {
            return Err(ProgramError::MissingRequiredSignature);
        }
        if !payer.is_writable() || !pda.is_writable() {
            return Err(ProgramError::Immutable);
        }
        if !pubkey_eq(pda.key(), &PDA_V1) {
            return Err(ProgramError::InvalidSeeds);
        }
        if !pubkey_eq(pda.owner(), &pinocchio_system::ID) {
            return Err(ProgramError::AccountAlreadyInitialized);
        }
        if !pubkey_eq(system_program.key(), &pinocchio_system::ID) {
            return Err(ProgramError::IncorrectProgramId);
        }

        Ok(Self {
            payer,
            pda,
            system_program,
        })
    }
}
