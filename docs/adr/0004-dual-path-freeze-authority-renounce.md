# `freeze_authority_renounce` accepts either admin or freeze_authority self-renounce

`freeze_authority_renounce` is authorized by EITHER the current admin (per RFP-002 F5) OR the current freeze_authority self-renouncing. Both signature paths are checked in the same handler via an OR. The handler reads both `admin_config` and `freeze_config` to perform the check.

The proposal (line 56) specified admin-only: "`revoke_freeze_authority()` - sets freeze_authority to None. Admin only. Permanent." We add the self-renounce path because admin-authority's renounce is terminal — if a consumer renounces admin before freeze, admin-only renounce would leave the freeze authority slot permanently occupied until the keyholder loses their key (the same accidental-renounce-by-key-loss admin-authority warns against, reached without explicit intent here).

F5 says "Freeze authority can be revoked by admin authority" — capability requirement, not exclusivity requirement. Additional auth paths are not forbidden.

## Considered Options

**1. Dual-path: admin OR freeze_authority self (chosen).**
Two auth checks ORed. Both `admin_config` and `freeze_config` read in the handler. Single instruction (`freeze_authority_renounce`), no signature change.
Cost: handler reads two PDAs instead of one. Slightly larger tx-size overhead. README must document that the freeze authority itself can step down.

**2. Admin-only (proposal-faithful).**
Single auth check. Simpler handler.
Rejected because the edge case (admin renounced first → freeze authority orphaned) is real, rare, and irreversible if hit. Self-renounce is a cheap insurance policy.

**3. Separate instructions.**
`freeze_authority_renounce_by_admin` and `freeze_authority_renounce_self` as two distinct instructions.
Rejected because the IDL surface doubles with no semantic gain. Same end state; same data write.

## Consequences

- `freeze_authority_renounce` takes both `admin_config` and `freeze_config` as account params. Slightly larger tx.
- README documents both paths and the rationale (admin-renounced-first edge case).
- F5 remains satisfied: admin can still revoke. The additional self-renounce path is additive, not subtractive.
- Future audit consideration: an attacker who compromises only the freeze_authority key can renounce the freeze slot. Per ADR-0007, this is recoverable by admin (transfer to a new authority repopulates the slot). The attacker's blast radius is "force admin to rotate", not "permanently disable freeze".

## Semantic interaction with ADR-0007

ADR-0007 changes `freeze_authority_renounce`'s semantic from "terminal removal" to "vacate the slot". The dual-path authorization from this ADR survives the change without modification:

- **Admin path of renounce** under ADR-0007: admin vacates the slot. They can later refill via `freeze_authority_transfer` or leave it vacant. Equivalent to admin calling `transfer(default)` if such a thing were allowed — but renounce is more explicit at the call site.
- **Self path of renounce** under ADR-0007: freeze_authority steps down voluntarily. Admin can then assign a new one or leave the slot vacant.

Both paths leave the slot in the same Renounced state; the difference is purely who signed. ADR-0007's recovery path (admin transfer) is open regardless of which path was used to enter Renounced.
