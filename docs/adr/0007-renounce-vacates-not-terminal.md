# `freeze_authority_renounce` vacates the slot; admin can re-set via transfer

`freeze_authority_renounce` writes `AccountId::default()` into `FreezeConfig.freeze_authority`, transitioning the slot to a vacant state. From there, admin can call `freeze_authority_transfer` with a new candidate to repopulate the slot. The Renounced state is recoverable — not terminal.

This diverges from `admin-authority`'s renounce semantic (terminal). The divergence is principled: admin-authority has no higher authority to recover; freeze-authority has admin. Forcing terminal renounce on freeze would remove a capability admin should have.

The proposal (line 56) called revoke "Permanent." RFP-002 F5 just says "revoked by admin authority" without specifying permanence. The requirements doc allows either reading.

## Considered Options

**1. Renounce vacates; admin re-sets via transfer (chosen).**
Renounce zeros the slot. `freeze_authority_transfer` accepts the Renounced state and overwrites. Admin governs the slot's full lifecycle.

**2. Renounce is terminal (proposal-faithful, matches admin-authority).**
Once renounced, no future freeze authority. Recovery impossible.
Rejected because:
- Admin governs freeze. Terminal removes a capability admin should have.
- The "no future freeze authority" commitment is already achievable by never calling `freeze_initialize`. Terminal renounce doesn't unlock a unique capability.
- Operationally inflexible: a temporarily-vacant freeze role (e.g., between authority rotations) is impossible.

**3. Hybrid: two operations.**
`freeze_authority_renounce` (terminal, both admin and self can call) AND `freeze_authority_vacate` (admin-only, reversible by transfer). Two semantics, two instructions, larger glossary.
Rejected because:
- Larger IDL surface for marginal benefit. Most consumers will want one of the two paths.
- "Terminal" capability rarely needed; "vacate" suffices for the common case.

## Consequences

- `freeze_authority_transfer` validation drops the "freeze_config not Renounced" check. Admin can transfer over a Renounced state.
- `freeze_authority_renounce` still callable by admin or freeze_authority self per ADR-0004. Semantic shifts: from "terminal removal" to "vacate the slot; admin may repopulate later".
- Lifecycle state machine: Renounced is no longer a terminal sink. New transition: Renounced → Initialized via `freeze_authority_transfer` (admin-only).
- ADR-0004 unchanged in instruction set; semantics narrowed: self-renounce now means "step down voluntarily; admin can rotate or leave vacant".
- Admin-renounced-first edge case stronger: if admin is renounced first, the Renounced freeze slot is permanently unrecoverable (no admin to transfer in). This is the same end state but reached via a different path. Documented in lifecycle.md.
- New test: `freeze_authority_transfer_succeeds_over_renounced` confirms admin can recover the slot.
- The "Renounce is terminal" wording in earlier doc drafts is replaced everywhere with "Renounce vacates".
