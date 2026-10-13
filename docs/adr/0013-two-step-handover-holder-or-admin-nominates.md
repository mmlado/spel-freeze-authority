# Freeze authority two-step handover: the holder or the admin nominates, the nominee accepts

**Status:** accepted.

Freeze authority gets the same two-step handover as admin-authority (admin-authority ADR-0011), on the same spel-authority nomination (spel-authority ADR-0002), and keeps `freeze_authority_transfer` unchanged:

- `freeze_authority_nominate(new_freeze_authority: AccountId)`: the current freeze authority or the admin signs. The nominee is stored in the freeze slot.
- `freeze_authority_accept()`: the nominee signs alone and becomes freeze authority. A key nominee needs a program owner (`UnownedSigner`). A PDA nominee accepts through a chained call from its owning program.
- Both are `#[freeze_exempt]`, like transfer and renounce, so the role can change hands while the program is frozen.

Who may nominate follows the dual path of `freeze_authority_renounce` (ADR-0004): the holder can hand over to a successor without involving the admin, and the admin, which governs the slot (ADR-0007), can appoint one too. With the slot vacant only the admin can nominate, since there is no holder to sign, and accepting fills the vacant slot as `freeze_authority_transfer` already can.

## Considered Options

**1. Holder or admin (chosen).** Routine rotation needs only the holder, and the admin keeps every power it has today.

**2. Admin only.** Same gate as `freeze_authority_transfer`. Rejected: every rotation of the freeze key would need the admin's signature.

**3. Holder only.** The literal mirror of admin-authority. Rejected: the admin could not appoint through the two-step path, and a vacant slot could only be filled by the co-signed transfer.

## Consequences

- A later nomination replaces an earlier one, from either side. The admin keeps the last word: it can nominate over the holder's nominee, and it can renounce the holder (ADR-0004), which clears any nomination.
- `FreezeConfig` grows from 33 to 65 bytes. Embedded consumers move every field after it.
- admin-authority's `admin_nominate` and `admin_accept` join `wrap_instructions.exempt`.
