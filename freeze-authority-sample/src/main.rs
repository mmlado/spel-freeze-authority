use admin_authority::require_admin;
use freeze_authority::{FreezeCandidate, freeze_exempt};
use spel_framework::prelude::*;

#[account_type]
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Default)]
pub struct ProgramConfig {
    pub value: u64,
}

impl ProgramConfig {
    /// Loads state from an account's data field.
    fn from_account(account: &AccountWithMetadata) -> Result<Self, SpelError> {
        Self::try_from_slice(&account.account.data).map_err(|_| SpelError::SerializationError {
            message: "decoding failed".into(),
        })
    }

    /// Serialises and writes this state into an account's data field.
    fn write_to(&self, account: &mut AccountWithMetadata) -> Result<(), SpelError> {
        account.account.data = borsh::to_vec(self)
            .map_err(|_| SpelError::SerializationError {
                message: "encoding failed".into(),
            })?
            .try_into()
            .map_err(|_| SpelError::SerializationError {
                message: "data too large".into(),
            })?;
        Ok(())
    }
}

#[lez_program]
#[admin_authority]
#[freeze_authority]
mod freeze_authority_sample {
    /// Initialises the `program_config` PDA. Admin-gated so only the
    /// admin can bring the program up. Freeze-exempt because
    /// `freeze_config` does not yet exist at this point in the deploy
    /// sequence; without the exemption the auto-wrap prologue would try
    /// to decode `freeze_config` and reject with `NotInitialized`.
    #[require_admin]
    #[instruction]
    #[freeze_exempt]
    pub fn initialize(
        #[account(init, pda = literal("program_config"))] mut config: AccountWithMetadata,
    ) -> SpelResult {
        ProgramConfig::default().write_to(&mut config)?;
        // The gate's admin_config and caller params arrive by injection.
        Ok(SpelOutput::execute(vec![config], vec![]))
    }

    /// Auto-gated. Framework prepends `require_not_frozen`, so this
    /// rejects with `Frozen` when the program is frozen or `AccountFrozen`
    /// when the caller's per-account PDA is frozen.
    #[instruction]
    pub fn update_value(
        #[account(mut, pda = literal("program_config"))] mut config: AccountWithMetadata,
        new_value: u64,
    ) -> SpelResult {
        let mut state = ProgramConfig::from_account(&config)?;
        state.value = new_value;
        state.write_to(&mut config)?;
        Ok(SpelOutput::execute(vec![config], vec![]))
    }

    /// Freeze-exempt: stays callable even when the program is frozen.
    /// Emergency-read pattern.
    #[instruction]
    #[freeze_exempt]
    pub fn read_value(
        #[account(pda = literal("program_config"))] config: AccountWithMetadata,
    ) -> SpelResult {
        let _state = ProgramConfig::from_account(&config)?;
        Ok(SpelOutput::execute(vec![config], vec![]))
    }
}
