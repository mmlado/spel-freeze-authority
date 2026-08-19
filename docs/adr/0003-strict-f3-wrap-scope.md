# Strict F3: auto-wrap covers every dispatchable instruction, with declared exemptions

In auto mode, the framework hook wraps every instruction the consumer's program dispatches, including instructions injected by other extensions (admin-authority's three, future extensions). Instructions that must remain operable while frozen are declared in one of two ways:

1. **`#[freeze_exempt]` attribute on the instruction source.** freeze-authority's own management instructions self-declare via this attribute. The framework recognises it through the `self_exempt_marker = "freeze_exempt"` field in `[package.metadata.spel.wrap_instructions]`. Same attribute consumers use to opt out their own instructions — single mechanism, two callers.
2. **Metadata exempt list (`exempt = [...]`).** Cross-crate instructions whose source we don't control are named explicitly here. Used for admin-authority's three management instructions (which freeze-authority cannot modify per ADR-0001's "depend, don't fork" stance).

freeze-authority's instructions carrying `#[freeze_exempt]`:

- `freeze_initialize` — runs BEFORE `freeze_config` exists; wrapping would read an empty PDA and fail with `NotInitialized`, blocking the initial bootstrap.
- `freeze_program_release` — F3 carve-out: unfreezing must be callable while frozen.
- `freeze_authority_transfer` — F3 carve-out: changing the freeze authority must be callable while frozen.
- `freeze_authority_renounce` — F3 carve-out: vacating the slot must be callable while frozen.
- `freeze_account` — F3 carve-out: per-account freeze edits are useful prep work during a program-wide freeze.
- `freeze_account_release` — same as above.

`freeze_program` is NOT exempt. Wrapping it is semantically equivalent to the handler's own `AlreadyFrozen` idempotency check.

Metadata `exempt` list (cross-crate):

- `admin_authority::admin_initialize`
- `admin_authority::admin_transfer`
- `admin_authority::admin_renounce`

This is a strict reading of RFP-002 F3 ("rejecting any attempt to interact with it; apart from unfreezing or changing the freeze authority"). The proposal framed the library as a circuit-breaker pause that only blocks state-changing application logic; the requirements doc reads stronger ("any attempt to interact"). We follow the requirements: strict program-wide freeze with explicit carve-outs.

## Considered Options

**1. Strict F3, exemptions split between attribute (self) and metadata list (cross-crate) (chosen).**
Auto-wrap is universal. freeze-authority's own instructions self-declare via `#[freeze_exempt]`; cross-crate exemptions live in metadata. This minimises the metadata list to instructions whose source we don't control.
Cost: freeze-authority's metadata still names admin-authority's instructions explicitly, a soft coupling. Future extensions that should be exempt while frozen face the same coupling unless their authors adopt `#[freeze_exempt]` themselves.

**2. Narrow scope: wrap only consumer-authored fns.**
Cross-crate dispatched instructions escape by default. Each extension that wants to be gated when frozen opts in via its own metadata.
Rejected because:

- F3 reads strict in plain English. Defending an app-logic-only interpretation requires reframing "any attempt to interact" as "any application-logic call".
- A consumer using some random extension that didn't opt in would have those instructions callable while frozen → silent F3 leak.
Reopenable: if the RFP-002 committee clarifies F3 as app-logic-only, switch to this option; the rest of the design is unchanged.

**3. All exemptions in metadata (no self-declaration via attribute).**
freeze-authority's own seven instructions also listed in the `exempt` metadata array.
Rejected because:

- Renames in freeze-authority would require simultaneous metadata edits — error-prone.
- The `#[freeze_exempt]` attribute already exists for consumer opt-outs; reusing it for self-declaration is free.
- Self-declaration keeps the metadata list focused on its real job: cross-crate carve-outs we can't reach with an attribute.

**4. Strict F3, each extension self-declares its exempt status via its own metadata.**
admin-authority adds `wrap_instructions.always_exempt = true` (or similar) to its own metadata.
Rejected because:

- Couples admin-authority to freeze-authority's existence. Admin shouldn't know freeze exists.
- Future extensions face the same coupling.
- freeze-authority owning the cross-crate list is a single coordination point, acceptable for a security primitive.

## Consequences

- freeze-authority's metadata explicitly references admin-authority's instruction names. Maintenance burden when admin-authority's API evolves — soft coupling acknowledged.
- Future extensions need triage when they join the ecosystem. Convention: if an extension's instructions should be operable while frozen and the extension author cooperates, the extension adds `#[freeze_exempt]` to its sources (zero metadata churn here); otherwise freeze-authority's metadata `exempt` list adds them explicitly. README documents the convention.
- `freeze_initialize` is exempt for a different reason than the F3 carve-outs — it must run BEFORE `freeze_config` exists. This is captured in the per-instruction list above rather than buried as an implementation detail; future maintainers reading this ADR will know why the attribute is there even though F3 doesn't strictly require it.
- "Exempt is shallow" subtlety (see CONTEXT.md): a `#[freeze_exempt]` consumer fn that uses `chained_call` to invoke a gated fn still hits the gated fn's check. README documents this.
- Deadlock edge case: if admin loses their key while frozen, admin-authority's `admin_transfer` cannot recover (since it requires admin sig), but it remains callable. Mitigated by ADR-0004 (dual-path freeze renounce gives the freeze authority an exit) and by recommending consumers do not freeze without a recovery plan.
