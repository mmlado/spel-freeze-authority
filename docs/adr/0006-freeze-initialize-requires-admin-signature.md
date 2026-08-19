# `freeze_initialize` requires admin signature

`freeze_initialize` carries two signatures: the current admin authorizing the freeze setup, and the new freeze authority co-signing to accept the role (via `FreezeCandidate::Signer` evidence). The instruction validates the admin signature against `admin_config.admin` using `admin_authority::AdminConfig::assert_admin`.

The proposal (line 53) only specified "creates the config PDA. Re-init rejected automatically via #[account(init)]" without naming a required caller. Strengthening per RFP-002 F2's symmetry: if admin changes the freeze authority post-init, admin should also set it at init.

## Considered Options

**1. Admin-signed `freeze_initialize` (chosen).**
Required signatures: admin + new freeze authority. Closes the front-running window between deployment and freeze init. Symmetric with F2 (admin authorizes freeze authority changes).

**2. Open `freeze_initialize` (admin_config existence enforced, no admin signature).**
What ADR-0001 originally implied. Anyone can call as long as `admin_config` exists.
Rejected because:
- Leaves a front-running window: after `admin_initialize` succeeds, any caller can race to `freeze_initialize` and become the freeze authority before the legitimate admin can.
- Asymmetric with F2: admin controls every freeze authority CHANGE but not the SETTING.
- The single-tx setup pattern works but is an unenforced convention. Admin-signed init makes the guarantee load-bearing rather than docs-only.

**3. Open `freeze_initialize` with admin sig optional.**
Caller can be anyone if they pass admin's signature; otherwise rejected. Same effect as Option 1 but with optional sig field.
Rejected because making the security guarantee depend on optional fields is confusing. The strict signature requirement (Option 1) is clearer.

## Consequences

- `freeze_initialize` signature gains `#[account(signer)] caller: AccountWithMetadata`.
- admin-authority's `#[require_admin]` gate validates `caller` against `admin_config.admin` before the handler body runs.
- Edge case: admin must exist AND not be renounced when `freeze_initialize` runs. If admin has already been renounced, freeze can never be initialized for that program. Acceptable — if admin is gone, security primitives shouldn't be settable anyway.
- The recommended deployment pattern remains a single tx with `admin_initialize` followed by `freeze_initialize`. With this ADR, the freeze leg's safety no longer depends on the operator's discipline; the admin signature requirement enforces it.
- Asymmetric with admin-authority's `admin_initialize`, which is open (anyone can call). Defensible because admin has no prerequisite authority; freeze has admin as a prerequisite, so requiring admin signature is consistent with the hard-dep architecture.
- Test surface adds: `freeze_initialize_rejects_no_admin_sig`, `freeze_initialize_rejects_wrong_admin_sig`, `freeze_initialize_rejects_renounced_admin`.
