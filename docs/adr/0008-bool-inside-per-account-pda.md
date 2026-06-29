# Per-account freeze PDA stores `is_frozen: bool` (existence-only encoding is impossible)

`FrozenAccountState` is a 1-byte Borsh struct with a single `is_frozen: bool` field. The PDA at `(program_id, "frozen", target_account_id)` persists once created and is mutated in place by `freeze_account(target)` / `freeze_account_release(target)`. The auto-wrap gate prologue treats absent PDA (target was never frozen) and present-PDA-with-`is_frozen = false` identically — both pass the check.

Existence-only encoding (PDA exists ⇒ frozen; close ⇒ unfrozen; reinit on refreeze) was considered and rejected. Initially I framed this as a trade-off between rent at scale (favoring close+reinit) and lifecycle simplicity (favoring persistent). The M1 LEZ rent investigation made the decision unambiguous: existence-only is structurally impossible in LEZ.

## LEZ semantics that constrain the design

1. **No rent.** `Account` carries a `balance: u128` field but the validation rules enforce no minimum, no fee on creation. Storage is balance-free. Source: `nssa/core/src/account.rs:98-103`; grep for `rent|minimum_balance|lamport` in `nssa/` returns zero hits.
2. **No close primitive.** `validate_execution` rule 7 (`nssa/src/program.rs:621-629`) forbids any post-state where `program_owner` is `DEFAULT_PROGRAM_ID` while the pre-state was non-default. Once owned, permanently owned. The owner can zero balance and data, but the account record persists. There is no built-in refund or deallocate.
3. **Reinit on a "closed" account is impossible.** `Claim::Pda` requires `program_owner == DEFAULT_PROGRAM_ID` on the pre-state (`nssa/src/validated_state_diff.rs:210-214`). Per rule 7, this state is unreachable after first init. Reinit blocked by construction, not by policy.

## Considered Options

**1. Bool-inside PDA, persistent (chosen).**
`FrozenAccountState { is_frozen: bool }`. `freeze_account` inits if absent + writes `true`. `freeze_account_release` writes `false`. PDA persists.
Cost: storage grows monotonically with `targets_ever_frozen`. Acceptable because LEZ storage is balance-free (no rent accumulating against the freeze authority's wallet).

**2. Existence-only encoding.**
PDA exists ⇒ frozen; PDA absent ⇒ not frozen. `freeze_account` inits; `freeze_account_release` closes.
Rejected because LEZ has no close primitive (rule 7 above). Without close, there is no release path → F7 cannot be implemented.

**3. Bool-inside with explicit close on release.**
Same shape as Option 1 but `freeze_account_release` attempts to zero the PDA's storage as a soft "close".
Rejected because rule 7 prevents true close; zeroing data still leaves an owned account. Same end state as Option 1 (PDA persists) but with more code paths. Option 1 is simpler.

## Consequences

- Storage at the LEZ layer is append-only by design. Every `AccountId` that has ever been frozen retains a 1-byte PDA forever. At Solana-scale this would be a rent concern; in LEZ it's a non-issue.
- The PDA's owner stays freeze-authority's program throughout the lifecycle. Toggling `is_frozen` is a pure data mutation.
- F7 implementation: write `is_frozen = false`. No close, no reinit, no rent recovery needed.
- The `init_if_needed` SPEL semantic discussed in the lifecycle doc resolves to: `freeze_account(target)` declares `#[account(init, ...)]` on first call (PDA absent), `#[account(mut, ...)]` on subsequent calls. Two instruction variants (`freeze_account_create` and `freeze_account_update`) avoidable if SPEL `init_if_needed` works; either path lands the same on-chain shape. M2 PoC will confirm.
- Higher-layer storage fees (sequencer, settlement) are not visible at the nssa layer. Residual risk that future LEZ versions or higher protocol layers introduce per-account storage costs — out of scope for this design.
