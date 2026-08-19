//! CI consumer fixture: a realistic downstream program.
//!
//! Shaped like a real `spel init` consumer: a risc0 guest binary using the
//! admin and freeze markers, with the heavy dependency set (lee_core,
//! risc0-zkvm, ruint) that drags the arkworks stack into the lockfile.
//! The per-push `consumer-check` job host-checks this crate, which is the
//! proven trigger surface for the queued-lexer-diagnostic class of bug
//! (see the fix commit for `prefix Fp is unknown`). The scheduled
//! `consumer-build` job builds the real ELF, covering riscv-only classes.

#![no_main]

use admin_authority::require_admin;
use freeze_authority::{FreezeCandidate, freeze_exempt};
use spel_framework::prelude::*;

risc0_zkvm::guest::entry!(main);

#[account_type]
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug)]
pub struct ProgramConfig {
    pub value: u64,
}

#[lez_program]
#[admin_authority]
#[freeze_authority]
mod consumer_fixture {
    #[instruction]
    #[freeze_exempt]
    pub fn initialize(
        #[account(init, pda = literal("program_config"))] mut config: AccountWithMetadata,
        #[account(signer)] owner: AccountWithMetadata,
    ) -> SpelResult {
        let state = ProgramConfig { value: 0 };
        config.account.data = borsh::to_vec(&state)
            .map_err(|_| SpelError::SerializationError {
                message: "encoding failed".into(),
            })?
            .try_into()
            .map_err(|_| SpelError::SerializationError {
                message: "data too large".into(),
            })?;
        Ok(SpelOutput::execute(vec![config, owner], vec![]))
    }

    #[instruction]
    #[require_admin]
    pub fn update_value(
        #[account(pda = literal("admin_config"))] admin_config: AccountWithMetadata,
        #[account(signer)] caller: AccountWithMetadata,
        #[account(mut, pda = literal("program_config"))] mut config: AccountWithMetadata,
        new_value: u64,
    ) -> SpelResult {
        let state = ProgramConfig { value: new_value };
        config.account.data = borsh::to_vec(&state)
            .map_err(|_| SpelError::SerializationError {
                message: "encoding failed".into(),
            })?
            .try_into()
            .map_err(|_| SpelError::SerializationError {
                message: "data too large".into(),
            })?;
        Ok(SpelOutput::execute(
            vec![admin_config, caller, config],
            vec![],
        ))
    }
}
