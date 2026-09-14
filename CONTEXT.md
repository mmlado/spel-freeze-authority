# Freeze Authority

A SPEL library that brings the Freeze pattern, extended with a per-account blocklist, to LEZ programs. Source of truth: [PROPOSAL](https://github.com/logos-co/rfp/issues/47). Floor: [REQUIREMENTS](https://github.com/logos-co/rfp/blob/main/RFPs/RFP-002-freeze-authority-lib.md) (RFP-002).

## Language

**Program**:
A stateless ELF binary deployed to LEZ. Identified by its image_id. Each unique binary has a unique program_id and an isolated PDA namespace.
_Avoid_: contract, smart contract

**Admin** (reused from `admin-authority`):
The single `AccountId` stored in the `admin_config` PDA owned by the `admin-authority` library. Governs the freeze authority lifecycle (`freeze_authority_transfer`, `freeze_authority_renounce`). freeze-authority hard-depends on `admin-authority` and reads admin from `admin_config`. Proposal lines 33-34 conditional ("if RFP-001 delivered, depend") resolved to hard dep.
_See_: [admin-authority CONTEXT](https://github.com/mmlado/spel-admin-authority/blob/main/CONTEXT.md)

**Freeze authority**:
The single `AccountId` held in `FreezeConfig.slot` (a `authority::AuthoritySlot`), authorized to flip the program-wide frozen state and the per-account frozen state. Set at `freeze_initialize` (self-elected to the admin). Replaceable via `freeze_authority_transfer` (admin only). Vacatable via `freeze_authority_renounce` (dual-path: admin or current holder; recoverable by admin per ADR-0007, so not "permanent" — see below).
_Avoid_: pauser (ambiguous with LEZ vocabulary), freezer, "set_freeze_authority" or "revoke_freeze_authority" (proposal names; renamed for verb-style consistency with admin-authority)

**Freeze Config PDA**:
The on-chain account that stores `FreezeConfig` state. Derived from `(program_id, "freeze_config")`. Created once via `freeze_initialize`; reinit rejected.
_Avoid_: pause account, freeze account (overloaded with per-account freeze PDA)

**`FreezeConfig`**:
`{ slot: authority::AuthoritySlot, is_frozen: bool }`. The freeze authority `AccountId` lives inside `slot.holder()`, not as a direct field — `slot` wraps the shared `authority` crate's single-holder primitive. `slot.is_renounced()` is the renounced sentinel check — pattern-aligned with `admin-authority`'s `AdminConfig`, which embeds the same `AuthoritySlot`. Deviation from proposal line 53 (`Option<AccountId>`) for cross-library convention consistency. Fixed 33-byte Borsh encoding (32 AccountId + 1 bool) is preserved since `AuthoritySlot`'s only field borsh-encodes identically to a bare `AccountId`. Admin authority is NOT stored here — it lives in `admin_config` under the `admin-authority` library, accessed via `AdminConfig::assert_admin`.
_See_: [authority CONTEXT](https://github.com/mmlado/spel-authority/blob/main/CONTEXT.md) (AuthoritySlot, Renounced sentinel)

**`#[freeze_authority]` (auto, default)**:
Bare module-level attribute placed on a `#[lez_program]` module. Triggers framework discovery of the management instructions via `[package.metadata.spel.extension_attr]` AND triggers the auto-wrap framework hook (`[package.metadata.spel.wrap_instructions]`), which prepends the dual freeze gate (program-wide + signer's per-account PDA) to every dispatched instruction except those declared exempt. F3 and F6 conformance automatic.

**`#[freeze_authority(manual)]`**:
Module-level attribute with the explicit `manual` argument. Triggers framework discovery only; auto-wrap is skipped per the `skip = "manual"` entry in Cargo metadata. Consumer must annotate each instruction they want gated with `#[require_not_frozen]`. F3 conformance is the consumer's responsibility. Deviation from proposal: proposal's bare `#[freeze_authority]` was manual, `(auto)` was the opt-in; defaults are reversed here so the F3-conformant choice is the no-argument form. See ADR-0002.

**`#[require_not_frozen]`**:
Instruction-level opt-in for manual mode (`#[freeze_authority(manual)]`). Injects the dual freeze check (program-wide `freeze_config.is_frozen` AND the signer's per-account `freeze_account.is_frozen`, defaulting to `false` when the per-account PDA is missing) as a prologue at the top of the emitted handler by **re-expanding** on it — the framework leaves the attribute in place rather than stripping it (mechanism per admin-authority [ADR-0004](https://github.com/mmlado/spel-admin-authority/blob/main/docs/adr/0004-require-admin-injection-contract.md)). In auto mode the framework hook applies the same `require_not_frozen` proc-macro to every non-exempt instruction — one implementation, two callers.

Accepts kwargs `freeze_config`, `freeze_account`, and `caller` — one per inject-account declared in `[[package.metadata.spel.inject]]`. Framework auto-wrap emits every kwarg with the resolved param name so the prologue references whatever the consumer named their PDA and signer params. Manual mode may write the attribute bare (defaults `freeze_config` and `freeze_account`) or with kwargs pointing at custom names. Role-based remap in the framework detects three shapes and skips injection when the consumer already declared the role: the signer (`#[account(signer)]`), single-literal PDAs (`#[account(pda = literal("freeze_config"))]`), and compound-seed PDAs (`#[account(pda = [literal("frozen"), account("<signer>")])]`). Compound-seed comparison resolves the inner `account(...)` reference through the signer remap first, so a consumer's `sender` signer next to a `[literal("frozen"), account("sender")]` PDA still matches the spec's `[literal("frozen"), account("caller")]` shape. Unknown kwargs are a hard error (see [ADR-0010](docs/adr/0010-wrapper-args-are-inject-account-names.md)).

**`#[freeze_exempt]`**:
Instruction-level opt-out for `#[freeze_authority(auto)]` mode. Suppresses the auto-wrap for one consumer instruction. No-op inside manual mode. Also used inside `freeze-authority/src/lib.rs` on six management instructions to self-declare them exempt — the five F3 carve-outs plus `freeze_initialize`, which runs before `freeze_config` exists so the gate has nothing to read. The framework hook reads `self_exempt_marker = "freeze_exempt"` from Cargo metadata and skips any fn carrying the attribute. Only `freeze_program` is gated.

**Management instructions**:
Seven library-defined `#[instruction]` fns added via `#[freeze_authority]` discovery. All names prefixed with `freeze_`. Verb-pair form (no boolean toggles):

1. `freeze_initialize`
2. `freeze_program` — apply program-wide freeze
3. `freeze_program_release` — release program-wide freeze
4. `freeze_authority_transfer` — transfer the freeze authority slot
5. `freeze_authority_renounce` — vacates the freeze authority slot (recoverable by admin per ADR-0007)
6. `freeze_account(target)` — apply per-account freeze on `target`
7. `freeze_account_release(target)` — release per-account freeze on `target`

Source lives in [freeze-authority/src/lib.rs](https://github.com/mmlado/spel-freeze-authority/blob/main/freeze-authority/src/lib.rs); framework emits cross-crate dispatch into the consumer's binary. Deviation from proposal: proposal listed 5 instructions with `set_frozen(bool)`, `set_freeze_authority`, `revoke_freeze_authority`, `freeze_account(target, bool)`. Renamed for admin-authority alignment (verb-style, no booleans, no "revoke" — see [admin-authority CONTEXT](https://github.com/mmlado/spel-admin-authority/blob/main/CONTEXT.md)'s _avoid revoke_ rule) and uniform `freeze_` prefix.
_Avoid_: `set_frozen`, `set_freeze_authority`, `revoke_freeze_authority`, `freeze_account(_, bool)`.

**`FreezeCandidate`**:
`pub type FreezeCandidate = authority::AuthorityCandidate` — a type alias onto the shared `authority` crate's primitive, not a local duplicate. `Signer` carries no data; validation checks `new_freeze_account.is_authorized` (co-signed the tx). `Pda { program_id, seed }` validated by deriving the address via `AccountId::for_public_pda` and confirming the PDA is initialized. Distinct from `FreezeConfig.slot.holder()`, which stores only the resolved `AccountId`. Always paired with a `new_freeze_account: AccountWithMetadata` parameter; `FreezeCandidate` is the claim, `AccountWithMetadata` is the chain-state evidence.

Superseded: this used to be a local duplicate of `admin_authority::AdminCandidate` (same shape, same semantics, kept separate for IDL naming clarity and zero coupling to admin-authority's type evolution). Both admin-authority and freeze-authority now alias the same shared type from the `authority` crate instead, so there is no duplicated validation logic to maintain — IDL naming clarity is preserved by the type alias (`freeze_authority_transfer`'s candidate parameter still shows `FreezeCandidate`), and coupling is to `authority`, a policy-free crate neither library owns state in.
_See_: [authority CONTEXT](https://github.com/mmlado/spel-authority/blob/main/CONTEXT.md)
_Avoid_: using a bare `AccountId` arg for transfer (cannot validate key ownership or PDA existence).

**Per-account freeze target**:
Always the signer of the gated instruction. The proposal allowed an explicit `account = "target"` form for non-signer targets; dropped here for simplicity. Both modes check the signer's per-account freeze PDA — frozen account cannot call gated instructions on this program. A consumer that needs to gate a non-signer target writes the check manually inside their handler body — outside the macro.

**Auto-wrap scope (strict F3)**:
The framework's auto-wrap, when triggered by `#[freeze_authority]` bare or `#[freeze_authority(auto)]`, gates every instruction the program dispatches except:

1. Library-defined freeze management instructions whose semantics require operability while frozen or before init (the F3 carve-outs: `freeze_initialize`, `freeze_program_release`, `freeze_authority_transfer`, `freeze_authority_renounce`, `freeze_account`, `freeze_account_release`). Note `freeze_program` is NOT on this list — refreezing an already-frozen program is caught loudly by the prologue with `SpelError::Frozen`.
2. admin-authority's three management instructions (`admin_initialize`, `admin_transfer`, `admin_renounce`).
3. Consumer instructions explicitly marked `#[freeze_exempt]`.

The first set self-declares via `#[freeze_exempt]` (the `self_exempt_marker`), joined by `freeze_initialize` which is exempt because it runs before `freeze_config` exists. The second set is listed in `[package.metadata.spel.wrap_instructions].exempt` by qualified name. The third set is the consumer's own `#[freeze_exempt]` markers. Consumer-authored unmarked instructions are wrapped. Future extensions are wrapped by default unless freeze-authority's metadata adds them to the exempt list. Strict F3: when frozen, every dispatched instruction rejects except the carve-outs above. Stronger than a pause-style switch that only blocks state-changing app logic — chosen because the requirements doc reads strict.

**Exempt is shallow**:
The freeze gate is a prologue in the gated function's body. It runs when that function is invoked. A `#[freeze_exempt]` consumer fn that uses `chained_call` to invoke a gated fn still hits the gated fn's check — the gated fn rejects while frozen. Mitigation: exempt functions should call library helpers (plain Rust methods, no gate) or other exempt instructions. Calling admin-authority from exempt fns is always safe — admin instructions are exempt-listed and have no gate.

**Per-account freeze PDA**:
Derived from `(program_id, "frozen", target)`. Stores `{ is_frozen: bool }`. `freeze_account(target)` inits the PDA and writes `is_frozen = true`. `freeze_account_release(target)` writes `is_frozen = false`; PDA persists. Re-freezing the same account toggles the bool back without close+reinit. Auto-wrap reads-or-defaults: if the PDA does not exist (account never frozen), the macro treats the missing PDA as `is_frozen = false` and passes the check.

Persistent PDAs accumulate at O(N_ever_frozen). LEZ has no rent (balance-free storage per the M1 rent investigation), so this is a non-issue. Existence-only encoding was considered and rejected — LEZ has no close primitive and reinit after close is structurally impossible per validate_execution rule 7, so existence-only would have no F7 (release) path. Bool-inside is the only viable encoding.

**`freeze_initialize` signature**:
`freeze_initialize(admin_config, freeze_config, caller)`. Includes `#[account(pda = literal("admin_config"))]` to enforce admin is already initialized, `#[account(init, pda = literal("freeze_config"))]` to create the freeze config PDA, and `#[account(signer)] caller` validated against `admin_config.admin` per the `#[require_admin]` gate. The initial freeze authority is the admin themselves (self-elected via `FreezeCandidate::Signer` against `caller`) — admin can hand off to a distinct account later via `freeze_authority_transfer`. Deviation from proposal line 53 (`freeze_initialize(admin, freeze_authority)`): drops the `admin` parameter because admin-authority owns the admin state, and drops the explicit `new_freeze_authority` parameter to avoid a duplicate signer/account slot at init (admin_signer would be one signer, new_freeze_account would be a second). The admin signature closes the front-running window between `admin_initialize` and `freeze_initialize`. Recommended deployment: single tx with `admin_initialize` followed by `freeze_initialize` immediately after program deployment.

**`freeze_authority_renounce` authorization**:
Either the current admin (per RFP F5) or the current freeze_authority self-renouncing. Both signature paths accepted, in the same handler, via OR check. Reads both `admin_config` and `freeze_config` to verify. Deviation from proposal line 56 ("Admin only") for operational safety per ADR-0004. Additive auth path, RFP F5 still satisfied.

**Renounce semantic (ADR-0007)**:
`freeze_authority_renounce` vacates the freeze slot but is NOT terminal. Admin can re-populate via `freeze_authority_transfer` from the Renounced state. Distinct from admin-authority's terminal renounce — admin-authority has no higher authority to recover, freeze-authority has admin. Proposal line 56 called revoke "Permanent"; we deviate because admin should retain the capability to manage the slot's full lifecycle. The "no future freeze authority" commitment is recoverable by simply never calling `freeze_initialize` after admin is also renounced.

**Embedded freeze config (ADR-0011, ADR-0013)**:
`FreezeConfig` can live inside a consumer account. The consumer marks the embedding field with `#[freeze_slot]` and the account-creating instruction with `#[freeze_initialize]`, a no-op anchor that declares embedded mode, and the module marker stays bare. The offset derives from the marked field, there is no offset kwarg. The two declarations must agree, either one without the other refuses to build. Six management instructions take a trailing `offset: usize` filled by the framework at the dispatch call site, never by a caller. `freeze_initialize` the instruction is not emitted in embedded mode. Dedicated mode is the degenerate case offset 0, one code path. `FrozenAccountState` never embeds and stays offset free.

**Born vacant**:
An embedded freeze slot created without a bootstrap holds `AccountId::default()`, the vacant sentinel. It rejects every holder-path caller until the admin appoints a holder via `freeze_authority_transfer`. The appointment path and the renounce-recovery path are the same code. Distinct from admin's born renounced, which is permanent because admin has no higher authority.

**Cross-marker bound arg (ADR-0012)**:
`freeze_authority_transfer` and `freeze_authority_renounce` need admin's offset, which lives on the admin marker, not the freeze marker. The library declares `from = "admin_authority::offset"` in its bound_args metadata and the framework resolves the peer marker at the consumer's build, baking the value into the dispatch call as a literal. A missing peer marker or kwarg falls back to the declared default. A bound arg without a default makes both hard errors.

**Caller-decodes contract (ADR-0012)**:
A library method that reads peer state for an authorization decision takes the peer's decoded config by reference, `admin: Option<&AdminConfig>` for dual-path renounce, never the peer's account. The instruction body decodes at the top and passes the value in. This is what lets one shared account serve both roles without aliasing two borrows of one param.

**Shared-account merge**:
When admin and freeze embed into the same consumer account, a dual-role instruction's two role params resolve to one account. The framework lists it once in the IDL, enum, and validation, and the dispatcher clones the single transaction account into both positions of the precompiled call. The instruction body collapses its output through `post_state_pair`, one post-state per unique account id, satisfying the LEZ duplicate-account rule. Two embeds naming the same account at the same offset are a compile error.

**Body-level admin check**:
`freeze_authority_transfer` performs its strict admin check inside the body, decode at `admin_offset` then `assert_admin(caller)`. The `#[require_admin]` gate cannot serve it because gate attrs on library fns expand when the library compiles, before any consumer offset exists. `freeze_initialize` keeps its gate, it is dedicated-only and embedded mode skips it at discovery.
