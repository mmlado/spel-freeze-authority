use freeze_authority::{FreezeCandidate, require_not_frozen};
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
#[freeze_authority(manual)]
mod freeze_authority_sample_manual {
    use admin_authority::{AdminCandidate, AdminConfig};
    use freeze_authority::{FreezeCandidate, FreezeConfig};

    /// Combined deploy-time initialiser. Bootstraps `admin_config`,
    /// `freeze_config`, and `program_config` in a single call so the
    /// program is fully set up in one tx (per the LEZ post-deploy init
    /// pattern). Not annotated with `require_not_frozen`, so it runs
    /// ungated at bootstrap when `freeze_config` doesn't yet exist.
    ///
    /// Consumers who prefer the library's individual init instructions
    /// (`admin_initialize`, `freeze_initialize`) can drop those bootstraps
    /// from here and dispatch them separately; either shape works.
    #[instruction]
    pub fn initialize(
        #[account(init, pda = literal("admin_config"))] mut admin_config: AccountWithMetadata,
        #[account(init, pda = literal("freeze_config"))] mut freeze_config: AccountWithMetadata,
        #[account(init, pda = literal("program_config"))] mut config: AccountWithMetadata,
        #[account(signer)] signer: AccountWithMetadata,
    ) -> SpelResult {
        AdminConfig::bootstrap(&mut admin_config, AdminCandidate::Signer, &signer)?;
        FreezeConfig::bootstrap(&mut freeze_config, FreezeCandidate::Signer, &signer)?;
        ProgramConfig::default().write_to(&mut config)?;
        Ok(SpelOutput::execute(
            vec![admin_config, freeze_config, config, signer],
            vec![],
        ))
    }

    /// A representative gated instruction. Manual mode means the
    /// framework does NOT auto-prepend `#[require_not_frozen]` — the
    /// consumer applies it explicitly here. When the program is frozen
    /// the gate prologue rejects the call.
    #[instruction]
    #[require_not_frozen]
    pub fn update_value(
        #[account(mut, pda = literal("program_config"))] mut config: AccountWithMetadata,
        new_value: u64,
    ) -> SpelResult {
        let mut state = ProgramConfig::from_account(&config)?;
        state.value = new_value;
        state.write_to(&mut config)?;
        Ok(SpelOutput::execute(vec![config], vec![]))
    }

    /// Demonstrates the opposite end of manual mode: this instruction
    /// is NOT gated. The consumer chose not to annotate it with
    /// `#[require_not_frozen]`. F3 conformance becomes the consumer's
    /// responsibility — "selective gating by design" as the proposal
    /// describes. There is no `#[freeze_exempt]` needed; manual mode
    /// gates only what's annotated.
    #[instruction]
    pub fn read_value(
        #[account(pda = literal("program_config"))] config: AccountWithMetadata,
    ) -> SpelResult {
        let _state = ProgramConfig::from_account(&config)?;
        Ok(SpelOutput::execute(vec![config], vec![]))
    }
}
