use spel_framework::prelude::*;
use admin_authority::admin_authority;
use freeze_authority::{freeze_authority, freeze_exempt};

#[lez_program]
#[admin_authority]
#[freeze_authority]
mod freeze_authority_sample {
    use super::*;

    /// A representative gated instruction. Auto-mode means the framework
    /// prepends `#[require_not_frozen]` before this handler is generated;
    /// when the program is frozen the gate prologue rejects the call.
    #[instruction]
    pub fn update_value(
        #[account(mut, pda = literal("program_config"))] mut _config: AccountWithMetadata,
        _new_value: u64,
    ) -> SpelResult {
        todo!()
    }

    /// Demonstrates `#[freeze_exempt]`: this instruction stays callable
    /// while the program is frozen (e.g. an emergency withdraw). The
    /// framework reads `self_exempt_marker = "freeze_exempt"` from
    /// freeze-authority's Cargo metadata and skips this fn.
    #[instruction]
    #[freeze_exempt]
    pub fn read_value(
        #[account(pda = literal("program_config"))] _config: AccountWithMetadata,
    ) -> SpelResult {
        todo!()
    }
}
