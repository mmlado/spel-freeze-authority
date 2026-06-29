# Hard Cargo dependency on `admin-authority`

freeze-authority depends on `admin-authority` as a Cargo dependency. The admin role lives entirely in `admin-authority`'s `admin_config` PDA. `FreezeConfig` carries only `{ freeze_authority, is_frozen }` — no `admin` field, no `admin_enabled` field. Admin-side authorization (`freeze_authority_transfer`, admin-side of `freeze_authority_renounce`) delegates to `AdminConfig::assert_admin`.

The proposal hedged on this point (lines 33-34): "If RFP-001 already delivered, depend on it. If not, minimal admin authority will be included within this scope." RFP-001 is in flight (M2 in [spel-admin-authority/](../../../spel-admin-authority/)), no upstream PR merged. We commit to the dep regardless.

## Considered Options

**1. Hard Cargo dep on admin-authority (chosen).**
Single source of truth for the admin lifecycle. No duplicated state machine. Cross-library glossary stays consistent. `FreezeConfig` is small. Cost: freeze-authority cannot be used standalone; consumers must pull in both libraries.

**2. Inline minimal admin into `FreezeConfig`.**
Standalone library. `FreezeConfig` carries `{ admin, admin_enabled, freeze_authority, is_frozen }` and freeze-authority ships its own mini admin-init/transfer/renounce instructions.
Rejected because:
- Duplicates the admin state machine. Two admins on programs that use both libraries unless consumers manually keep them in sync. Confusing UX.
- Glossary divergence: `admin` overloaded across two libraries.
- Sync hazard: if admin-authority's admin transfers, freeze-authority's `admin` is stale and the new admin can't manage freeze until a second instruction runs.

**3. Optional Cargo feature flag.**
`--features admin-authority` enables the hard-dep path; disabled falls back to the inlined manager slot.
Rejected because two code paths double maintenance cost for a primitive whose semantics should be uniform. The "optional integration" use case can be addressed later (a future decoupling ADR) without committing to it now.

## Consequences

- `freeze_initialize` requires `admin_config` to exist (enforced by `#[account(pda = literal("admin_config"))]`). Recommended deployment: single tx with `admin_initialize` followed by `freeze_initialize`.
- If admin-authority's admin is renounced first, the admin-side of `freeze_authority_renounce` becomes permanently inert. Mitigated by ADR-0004 (dual-path renounce).
- freeze-authority is not shippable without admin-authority. Acceptable: the proposal treats them as a coordinated pair.
