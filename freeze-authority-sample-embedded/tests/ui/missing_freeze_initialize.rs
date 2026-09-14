// The account-creating instruction lacks #[freeze_initialize]. The
// slot is born vacant either way, but the marked field declares
// embedded mode with nothing anchoring it, and the framework must
// refuse the disagreement instead of compiling dedicated mode with a
// dead slot window.
use borsh::{BorshDeserialize, BorshSerialize};
use freeze_authority::FreezeConfig;
use spel_framework::prelude::*;

#[account_type]
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Default)]
pub struct ProgConfig {
    pub value: u64,
    pub padding: [u8; 24],
    #[freeze_slot]
    pub freeze: FreezeConfig,
}

#[lez_program]
#[freeze_authority]
mod fixture {
    #[instruction]
    pub fn initialize(
        #[account(init, pda = literal("prog_config"))] config: AccountWithMetadata,
        #[account(signer)] signer: AccountWithMetadata,
    ) -> SpelResult {
        Ok(SpelOutput::execute(
            vec![config.account, signer.account],
            vec![],
        ))
    }
}
