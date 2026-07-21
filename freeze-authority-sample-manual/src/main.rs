use freeze_authority::require_not_frozen;
use spel_framework::prelude::*;

#[lez_program]
#[admin_authority]
#[freeze_authority(manual)]
mod freeze_authority_sample_manual {
    /// A representative gated instruction. Manual mode means the
    /// framework does NOT auto-prepend `#[require_not_frozen]` — the
    /// consumer applies it explicitly here. When the program is frozen
    /// the gate prologue rejects the call.
    #[instruction]
    #[require_not_frozen]
    pub fn update_value(
        #[account(mut, pda = literal("program_config"))] mut _config: AccountWithMetadata,
        _new_value: u64,
    ) -> SpelResult {
        todo!()
    }

    /// Demonstrates the opposite end of manual mode: this instruction
    /// is NOT gated. The consumer chose not to annotate it with
    /// `#[require_not_frozen]`. F3 conformance becomes the consumer's
    /// responsibility — "selective gating by design" as the proposal
    /// describes. There is no `#[freeze_exempt]` needed; manual mode
    /// gates only what's annotated.
    #[instruction]
    pub fn read_value(
        #[account(pda = literal("program_config"))] _config: AccountWithMetadata,
    ) -> SpelResult {
        todo!()
    }
}
