# Authority Lifecycle

How freeze-authority's three state machines evolve over a program's lifetime, what each transition validates, and which guarantees the library provides.

freeze-authority models three independent-but-related states:

1. The **freeze authority slot** in `FreezeConfig.freeze_authority`. Lifecycle: Uninitialized → Initialized → Renounced.
2. The **program-wide frozen flag** in `FreezeConfig.is_frozen`. Boolean toggle. Only meaningful while the authority slot is Initialized.
3. The **per-account frozen state** in the per-account PDA at `(program_id, "frozen", target)`. One toggle per target `AccountId`.

The freeze authority slot governs both flags: the authority is the only role that can flip `is_frozen` and the per-account state (subject to admin-side rules for the slot itself).

## State machine — freeze authority slot

```text
        ┌──────────────────┐
        │  Uninitialized   │   freeze_config PDA does not exist
        └────────┬─────────┘
                 │ freeze_initialize (admin-signed)
                 ▼
        ┌──────────────────┐
        │   Initialized    │   freeze_authority = <set>
        │  fa = AccountId  │◄──────────┐
        └────┬─────────────┘           │
             ▲                         │ freeze_authority_transfer
             │ freeze_authority_       │   (admin-signed; works
             │   transfer              │    from Initialized OR
             │   (admin-signed)        │    Renounced state)
             │                         │
        ┌────┴─────────────┐           │
        │    Renounced     │───────────┘
        │  fa = default    │   admin can re-set via transfer
        └──────────────────┘   (ADR-0007; not terminal)
             ▲
             │ freeze_authority_renounce
             │   (admin OR freeze_authority self)
             │
        (from Initialized)
```

### States

**Uninitialized.** The `freeze_config` PDA at `(program_id, "freeze_config")` does not yet exist on-chain. The only caller who can submit `freeze_initialize` is the current admin (per ADR-0006); third parties cannot front-run. If `admin_config` is also Uninitialized, freeze cannot be initialized either — the precondition fails.

**Initialized.** The `freeze_config` PDA exists and `freeze_authority` holds the current authority's `AccountId`. Every freeze-gated instruction (program-wide and per-account) reads this value to authorize `freeze_program`, `freeze_program_release`, `freeze_account`, `freeze_account_release`.

**Renounced.** The `freeze_config` PDA exists but `freeze_authority` is `AccountId::default()`. Per ADR-0007, this is **NOT** a terminal state — admin can re-populate the slot via `freeze_authority_transfer`. While Renounced, all freeze-authority-signed operations (`freeze_program`, `freeze_program_release`, `freeze_account`, `freeze_account_release`) fail because there is no current authority; admin-signed operations (`freeze_authority_transfer` to repopulate, `freeze_authority_renounce` to confirm vacancy, no-op) succeed. `is_frozen` and the per-account PDAs retain whatever values they had at the moment of renounce.

### Transitions

#### `freeze_initialize`

```text
Uninitialized → Initialized
```

**Inputs:** `caller: AccountWithMetadata` (current admin, signing), `new_freeze_account: AccountWithMetadata` (claim subject), `new_freeze_authority: FreezeCandidate`.

**Account constraints:** `freeze_config` is `#[account(init, pda = literal("freeze_config"))]`; `admin_config` is `#[account(pda = literal("admin_config"))]` (must already exist — enforces the hard-dep precondition from ADR-0001).

**Authorization:** the admin signature is required per ADR-0006. admin-authority's `#[require_admin]` gate validates `caller` against `admin_config.admin` before the handler body runs. Closes the front-running window between `admin_initialize` and `freeze_initialize` — without this requirement, any third party could win the race and become the freeze authority.

**Resolution:**

- `FreezeCandidate::Signer`: `freeze_authority` is set to `new_freeze_account.account_id`. The new authority must co-sign the transaction (`is_authorized == true`) to accept the role.
- `FreezeCandidate::Pda { program_id, seed }`: the library derives the expected PDA address, confirms it matches `new_freeze_account.account_id`, and confirms the PDA is already deployed.

**Validations:** `admin_config` exists and is not Renounced. `caller` matches the current admin. The `freeze_config` PDA is freshly initialized (enforced by `#[account(init)]`). The resolved authority is not `AccountId::default()`.

**Initial values written:** `freeze_authority = <resolved>`, `is_frozen = false`.

**Failure modes:** `FreezeError::NotAdmin` (admin signer wrong/missing), `FreezeError::Renounced` (admin renounced before freeze ever initialized), `FreezeError::InvalidCandidate`, `FreezeError::UndeployedPda`, `FreezeError::CandidateMismatch`, `FreezeError::AlreadyInitialized` (if `freeze_config` already exists).

#### `freeze_authority_transfer`

```text
Initialized → Initialized (new authority)
```

**Inputs:** `caller: AccountWithMetadata` (the current admin, signing), `new_freeze_account: AccountWithMetadata`, `new_freeze_authority: FreezeCandidate`.

**Account constraints:** `freeze_config` `#[account(mut)]`; `admin_config` `#[account(pda = literal("admin_config"))]`.

**Validations:** `freeze_config` exists (not Uninitialized). Renounced state is acceptable per ADR-0007 — admin can re-populate. `admin_config` is not Renounced (delegated to `admin_authority::AdminConfig::assert_admin`). `caller` is the current admin per `assert_admin`. `FreezeCandidate::validate_with_account(new_freeze_account)` succeeds.

**Edge case — called before `freeze_initialize`:** the `freeze_config` PDA does not exist; `account.data` is empty; `FreezeConfig::from_account` returns `FreezeError::NotInitialized`. Caller cannot transfer a freeze authority that was never set.

**Edge case — called over Renounced state:** allowed per ADR-0007. Admin signature required. `freeze_authority` is overwritten with the resolved candidate. Slot transitions Renounced → Initialized.

**Failure modes:** `FreezeError::NotInitialized`, `FreezeError::NotAdmin`, `FreezeError::InvalidCandidate`, `FreezeError::UndeployedPda`, `FreezeError::CandidateMismatch`. (`Renounced` is NOT a failure mode for transfer — it's an acceptable starting state.)

#### `freeze_authority_renounce`

```text
Initialized → Renounced
```

**Inputs:** `caller: AccountWithMetadata` — must match EITHER the current admin OR the current freeze_authority. Dual-path per ADR-0004. The instruction carries `#[instruction] #[freeze_exempt]` without `#[require_admin]`; the OR check lives inside `FreezeConfig::renounce`, which tries the admin path first (`AdminConfig::from_account(admin_account).and_then(|c| c.assert_admin(current))`) and falls back to the freeze-authority holder path (`self.slot.assert(current)`) if the admin path fails.

**Account constraints:** `freeze_config` `#[account(mut)]`; `admin_config` `#[account(pda = literal("admin_config"))]`.

**Validations:**

- `freeze_config` must be Initialized (not already Renounced, not Uninitialized).
- `signer.is_authorized` must be true.
- `signer.account_id == admin_config.admin` OR `signer.account_id == freeze_config.freeze_authority`. If admin has already been Renounced (`admin_config.admin == AccountId::default()`), only the self-renounce path is available.

**Edge case — called before `freeze_initialize`:** same as transfer — empty PDA data returns `FreezeError::NotInitialized`.

**Effect:** writes `freeze_authority = AccountId::default()`. NOT terminal per ADR-0007 — admin can re-populate via `freeze_authority_transfer`. `is_frozen` and per-account PDAs are not modified.

**Failure modes:** `FreezeError::NotInitialized`, `FreezeError::NotAdmin` (signer matches neither role), `FreezeError::Renounced`, `FreezeError::MissingSignature`.

## State machine — program-wide frozen flag

```text
        ┌──────────┐  freeze_program             ┌──────────┐
        │ Unfrozen │  ──────────────────────►    │  Frozen  │
        │          │                              │          │
        │ false    │  ◄──────────────────────     │   true   │
        └──────────┘   freeze_program_release    └──────────┘
```

`FreezeConfig.is_frozen` is set to `false` at `freeze_initialize` and toggled by `freeze_program` / `freeze_program_release`. Both transitions require the current freeze_authority's signature.

### `freeze_program`

```text
Unfrozen → Frozen
```

**Inputs:** `freeze_signer: AccountWithMetadata` (must match current `freeze_authority`).

**Validations:** authority slot is Initialized (not Renounced, not Uninitialized); `freeze_signer.is_authorized`; `freeze_signer.account_id == freeze_config.freeze_authority`.

**Failure modes:** `FreezeError::NotInitialized`, `FreezeError::NotFreezeAuthority`, `FreezeError::Renounced`, `FreezeError::MissingSignature`, `FreezeError::AlreadyFrozen` (idempotency).

### `freeze_program_release`

```text
Frozen → Unfrozen
```

Mirror of `freeze_program`. Calling when already unfrozen returns `FreezeError::NotFrozen`.

## State machine — per-account frozen state

```text
              ┌──────────────────────┐
              │  Unfrozen (default)  │   PDA absent OR is_frozen = false
              │                      │
              └────┬────────────┬────┘
                   │            ▲
                   │ freeze_    │ freeze_account_
                   │  account   │   release
                   ▼            │
              ┌──────────────────────┐
              │       Frozen         │   PDA present, is_frozen = true
              │                      │
              └──────────────────────┘
```

Per-account state is keyed by `AccountId`. The PDA at `(program_id, "frozen", target)` stores `{ is_frozen: bool }`. Missing PDA is equivalent to `is_frozen = false`.

### `freeze_account(target)`

```text
Unfrozen → Frozen
```

**Inputs:** `freeze_signer: AccountWithMetadata` (must match current `freeze_authority`); `target: AccountWithMetadata`.

**Account constraints:** `frozen_pda` `#[account(mut, pda = [literal("frozen"), arg("target")])]`. The `mut` constraint handles both first-freeze (empty PDA data, initialised in place) and re-freeze (existing PDA, bool toggled) uniformly — no separate init path.

**Effect:** initializes the per-account PDA (or mutates an existing one) and writes `is_frozen = true`.

**Validations:** authority slot Initialized; `freeze_signer` matches `freeze_authority`; signature present.

**Failure modes:** `FreezeError::NotInitialized`, `FreezeError::NotFreezeAuthority`, `FreezeError::Renounced`, `FreezeError::AccountAlreadyFrozen` (idempotency).

### `freeze_account_release(target)`

```text
Frozen → Unfrozen
```

**Effect:** writes `is_frozen = false` into the existing per-account PDA. Does not close the PDA.

**Failure modes:** `FreezeError::NotInitialized`, `FreezeError::AccountNotFrozen` (target has no PDA, or PDA stores `false`).

## State detection

Two states have empty-looking config slots in different ways. Uninitialized means the `freeze_config` PDA contains no data; Renounced means it contains data but the freeze_authority field is zeroed.

| State         | `account.data` | `freeze_authority`     |
| ------------- | -------------- | ---------------------- |
| Uninitialized | empty          | n/a (decode fails)     |
| Initialized   | non-empty      | non-default            |
| Renounced     | non-empty      | `AccountId::default()` |

`FreezeConfig::from_account` plus `FreezeConfig::assert` discriminate in order:

1. `from_account` returns `FreezeError::NotInitialized` if `account.data` is empty.
2. `from_account` decodes into `FreezeConfig`, then `assert(signer)` returns `FreezeError::Renounced` when `slot.is_renounced()` (holder is `AccountId::default()`).
3. Otherwise `assert(signer)` compares `signer.account_id` to `slot.holder()`, returning `FreezeError::NotFreezeAuthority` on mismatch or `FreezeError::MissingSignature` when the witness set has not authorised the signer.

This three-way discrimination is what protects every management instruction from being called before `freeze_initialize`: any call against an Uninitialized state returns `NotInitialized` cleanly. No special-casing per instruction is needed.

Per-account state has its own three-way discrimination:

| State        | PDA presence | `is_frozen` |
| ------------ | ------------ | ----------- |
| Never frozen | absent       | n/a         |
| Frozen       | present      | `true`      |
| Released     | present      | `false`     |

The auto-wrap macro treats "absent" and "present-and-false" identically: not frozen, instruction allowed. Decoding only happens if the PDA is present.

## Reinit rejection

`freeze_config` is created with `#[account(init)]`. LEZ's `validate_execution` rule rejects any post-state where the pre-account was already non-default but the instruction declared `init`. So once `freeze_initialize` succeeds, no second call can succeed, even after renounce. The PDA address is fixed at `(program_id, "freeze_config")`, so there is no second address to initialize.

Per-account PDAs use `#[account(mut, pda = [literal("frozen"), arg("target")])]` uniformly. First-freeze against a target is a write into an empty account data slot; re-freeze toggles the bool. No separate init variant. The `FrozenAccountState::from_data_or_default` decoder treats empty bytes as `is_frozen = false`, so the first-freeze path is a plain write with no init ceremony.

The M1 LEZ rent investigation confirmed that closed-and-reinit cycles are impossible (LEZ rule 7 forbids `program_owner → DEFAULT`), so PDAs stay owned by freeze-authority for their lifetime. Per ADR-0008, releases mutate the bool in place rather than closing the PDA. Storage is balance-free, so persistent PDAs do not accumulate rent against the freeze authority.

## Signer validation

The `is_authorized` flag on `AccountWithMetadata` is set by LEZ during transaction validation. It is true if and only if the transaction's `WitnessSet` contains a valid signature over the tx body by the AccountId's keypair. Library methods that take a signer check this flag instead of re-implementing signature verification. SPEL's `#[account(signer)]` constraint emits the same check automatically before the handler runs; the library checks again at its own boundary to enforce the invariant defensively.

For `FreezeCandidate::Signer`, `new_freeze_account.is_authorized` must also be true. Without this, an attacker could name an arbitrary AccountId as the new authority. Requiring a co-signature proves the new authority's keyholder consents to the transfer.

For `FreezeCandidate::Pda`, signatures aren't applicable. PDAs cannot sign. The library proves the candidate by deriving the address from `(program_id, seed)`, checking it matches `new_freeze_account.account_id`, and checking the PDA has been deployed (`account != Account::default()` and `program_owner != DEFAULT_PROGRAM_ID`).

## Program-as-authority via CPI

Both `admin` and `freeze_authority` can be PDAs. The CPI pattern is the same for both roles:

The owning program builds a chained_call to the target freeze-authority instruction, includes the PDA in the call's account list, and declares `caller-pda-seeds = seed`. LEZ verifies that `AccountId::for_public_pda(caller_program_id, seed)` matches `PDA.account_id`. If it does, LEZ propagates `is_authorized = true` to the callee. The handler's identity check (`signer.account_id == admin_config.admin` or `signer.account_id == freeze_config.freeze_authority`) passes just as it would for an EOA signer.

**admin as PDA**: the owning program calls `freeze_initialize`, `freeze_authority_transfer`, or `freeze_authority_renounce`'s admin path with the admin PDA as the `caller` account. `AdminConfig::assert_admin` succeeds because LEZ has set `is_authorized = true` via the seed claim.

**freeze_authority as PDA**: the owning program calls `freeze_program`, `freeze_program_release`, `freeze_account`, `freeze_account_release`, or the self-path of `freeze_authority_renounce` with the freeze PDA as the `freeze_signer` account. `FreezeConfig::assert` succeeds.

Only the owning program can produce a valid seed claim, because LEZ pins `caller_program_id` to the actual caller. PDA candidates passed to `freeze_initialize` or `freeze_authority_transfer` (via `FreezeCandidate::Pda`) must already be deployed; otherwise validation rejects.

## Initialization window

Between deployment and the first successful `admin_initialize`, the admin slot is Uninitialized — anyone can submit `admin_initialize` and become admin. This is admin-authority's known initialization window. freeze-authority inherits it as a precondition: freeze cannot be initialized until admin exists.

Between `admin_initialize` and `freeze_initialize`, no front-running of the freeze slot is possible. Per ADR-0006, only the current admin can call `freeze_initialize`. The admin signature requirement closes the freeze-side window.

Recommended deployment pattern: a single transaction containing `admin_initialize` followed by `freeze_initialize`, published immediately after program deployment. This minimizes the admin-side window to zero and uses ADR-0006 to keep the freeze-side closed.

## Renounce vacates the slot

`freeze_authority_renounce` writes `AccountId::default()` to `freeze_authority`. Per ADR-0007, this is a vacancy, not a terminal removal. Admin retains the capability to call `freeze_authority_transfer` and repopulate the slot with a new authority. This diverges from admin-authority's terminal renounce: admin-authority has no higher authority to recover, so renounce there must be terminal; freeze-authority has admin, so renounce vacates and admin can re-set.

Programs that need a temporary stand-down should use `freeze_program_release` instead — that flips `is_frozen` back to `false` without touching the authority slot. Use renounce when the freeze authority should step down and the slot may stay vacant until admin reassigns.

If the freeze_authority loses their key before renouncing or rotating, the freeze management surface (`freeze_program` / `freeze_program_release` / `freeze_account` / `freeze_account_release`) becomes permanently uncallable from that key. The current `is_frozen` and per-account states persist. Recovery: admin calls `freeze_authority_transfer` to a new authority, then the new authority manages from there. The lost-key case is not a renounce — the slot stays Initialized with the dead key — but the recovery path is identical (admin transfer to a new authority).

If the consumer wants to commit to "no future freeze authority ever", they should simply not call `freeze_initialize`. Once admin is renounced and freeze is vacant, the program reaches the same terminal state without needing a "terminal renounce" capability.

## Edge case — admin renounced first

admin-authority's `admin_renounce` is terminal. If a consumer renounces admin before renouncing freeze (or before transferring freeze), the admin-side paths become permanently inert:

| Operation                                  | Status after admin renounce           |
| ------------------------------------------ | ------------------------------------- |
| `freeze_authority_transfer`                | **dead** (requires admin sig)         |
| `freeze_authority_renounce` (admin path)   | **dead** (requires admin sig)         |
| `freeze_authority_renounce` (self path)    | **alive** — freeze_authority can exit |
| `freeze_program`, `freeze_program_release` | alive                                 |
| `freeze_account`, `freeze_account_release` | alive                                 |

The dual-path renounce (ADR-0004) is what keeps the freeze_authority's exit available. Without it, an admin-first renounce would orphan the freeze slot until the keyholder dies.

Note: after admin is renounced AND freeze_authority self-renounces, the freeze slot is Renounced with no admin to repopulate it. Effectively terminal at the system level, even though ADR-0007 makes Renounced normally recoverable. The recoverability depends on admin being alive. Renouncing admin first is the only way to make the freeze Renounced state permanent — and that requires a consumer choice to renounce admin.

## Edge case — admin renounced before freeze ever initialized

If `admin_renounce` runs before `freeze_initialize`, the program enters a terminal "freeze permanently unreachable" state. `freeze_initialize` requires admin signature per ADR-0006 and admin is gone. No one can ever set a freeze authority. Programs requiring freeze MUST initialize freeze before renouncing admin.

## Edge case — freeze_authority renounces while frozen

Allowed. The renounce transition writes only to `freeze_authority`; `is_frozen` is unchanged. Result: the program is frozen with a vacant freeze slot. Under ADR-0007, this is recoverable — admin can call `freeze_authority_transfer` to install a new authority, and the new authority can then call `freeze_program_release` to unfreeze. If admin is also gone, the frozen state becomes permanent (see "admin renounced first" edge case for the combined state).

## Edge case — calling any non-init management instruction before `freeze_initialize`

Every management instruction except `freeze_initialize` declares `freeze_config` with `#[account(mut, pda = ...)]` (not `init`). SPEL passes an empty `Account::default()` if the PDA hasn't been created. `FreezeConfig::from_account` checks `account.data.is_empty()` and returns `FreezeError::NotInitialized`. The behaviour is uniform across `freeze_authority_transfer`, `freeze_authority_renounce`, `freeze_program`, `freeze_program_release`, `freeze_account`, `freeze_account_release`. No instruction-specific handling needed.

## State invariants

- `freeze_authority` is either `AccountId::default()` (Renounced) or matches a valid AccountId (Initialized).
- `is_frozen` is independent of authority state — frozen + renounced is a valid combination (admin can recover via transfer + the new authority calls release).
- Per-account PDA presence is independent of authority state. Once frozen, the per-account state survives authority renounce.
- `freeze_initialize` requires `admin_config` to exist AND admin signature (hard-dep enforced by `#[account(pda)]`; admin-sig enforced by `AdminConfig::assert_admin` per ADR-0006).
- `freeze_authority_transfer` accepts both Initialized and Renounced starting states per ADR-0007. Admin can repopulate a Renounced slot.
- Any non-init management instruction called before `freeze_initialize` returns `FreezeError::NotInitialized` via the empty-data discriminator in `FreezeConfig::from_account`.
- Both `admin` and `freeze_authority` can be PDAs. CPI propagates `is_authorized = true` via seed claim; identity check is uniform across EOA and PDA cases.
