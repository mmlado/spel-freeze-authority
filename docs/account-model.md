# Account Model

What freeze-authority stores on-chain. Two account types, each created via SPEL's `#[account_type]` macro and serialized with Borsh.

For state transitions and lifecycle rules, see [authority-lifecycle.md](authority-lifecycle.md). This doc only describes the static shapes.

## `FreezeConfig`

One per program. Composes the shared `authority::AuthoritySlot` (which holds the freeze authority identity) with the program-wide frozen flag.

```rust
#[account_type]
#[derive(BorshSerialize, BorshDeserialize)]
pub struct FreezeConfig {
    slot: authority::AuthoritySlot,   // { holder: AccountId }
    pub is_frozen: bool,
}
```

The freeze authority `AccountId` lives inside `slot.holder()`, not as a direct field. `AuthoritySlot` is the shared single-holder primitive from `spel-authority`; the same type is embedded in `admin-authority`'s `AdminConfig`.

**PDA derivation:** `(program_id, "freeze_config")`. Single-seed PDA. Address is deterministic per program. Reinit rejected after first `freeze_initialize` call per LEZ's `validate_execution` rule.

**Encoding:** fixed 33 bytes — `AuthoritySlot` borsh-encodes to a single 32-byte `AccountId` (its only field), followed by the 1-byte bool. Borsh layout is direct: no length prefix, no discriminator, no padding. The composition is zero-cost on the wire; on-chain data shape matches a hand-rolled `{ AccountId, bool }` byte-for-byte.

**Sentinel for renounced:** `slot.is_renounced()` returns `true` when `slot.holder() == AccountId::default()`. Pattern-aligned with `admin-authority`'s `AdminConfig`, which uses the same slot primitive. Per ADR-0007, Renounced is recoverable by admin via `freeze_authority_transfer`.

### State detection

| State         | `account.data` | `slot`                                       |
| ------------- | -------------- | -------------------------------------------- |
| Uninitialized | empty          | n/a (decode fails)                           |
| Initialized   | non-empty      | `slot.holder()` is a non-default `AccountId` |
| Renounced     | non-empty      | `slot.is_renounced() == true`                |

Discrimination order in `FreezeConfig::from_account`:

1. If `account.data.is_empty()` → `FreezeError::NotInitialized`.
2. If decode succeeds and `slot.is_renounced()` → state is Renounced. Callers choose how to handle (`assert` returns `FreezeError::Renounced`; transfer accepts and overwrites).
3. Otherwise `slot.assert(signer)` compares `signer.account_id` to `slot.holder()` for authorization.

## `FrozenAccountState`

One per frozen `AccountId`. Stores the per-account frozen flag.

```rust
#[account_type]
#[derive(BorshSerialize, BorshDeserialize)]
pub struct FrozenAccountState {
    pub is_frozen: bool,
}
```

**PDA derivation:** `(program_id, "frozen", target_account_id)`. Multi-seed PDA. SPEL's `#[account(pda = [literal("frozen"), arg("target")])]` syntax derives the address at runtime from another param (the target). Supported by `spel-framework-macros` multi-seed PDA parsing.

**Encoding:** fixed 1 byte — single bool. Smallest possible per-account state. ADR-0008 (existence-only encoding) was considered for further savings but rejected — proposal commits to bool-inside.

**No sentinel; absence is the default state.** Per ADR-0008: if the PDA doesn't exist on-chain (target was never frozen), the auto-wrap macro treats the missing PDA as `is_frozen = false`. Same treatment as a present PDA storing `false` (a previously-frozen account that was released).

### State detection

| State        | PDA presence | `is_frozen` value |
| ------------ | ------------ | ----------------- |
| Never frozen | absent       | n/a               |
| Frozen       | present      | `true`            |
| Released     | present      | `false`           |

The gate prologue emitted by `require_not_frozen` (source lives in `freeze-authority-macros`):

```rust
let __cfg = ::freeze_authority::FreezeConfig::from_account(&#freeze_config_ident)?;
if __cfg.is_frozen { return Err(FreezeError::Frozen.into()); }

let __fa = ::freeze_authority::FrozenAccountState::from_data_or_default(
    &#freeze_account_ident.account.data,
)?;
if __fa.is_frozen { return Err(FreezeError::AccountFrozen.into()); }
```

Under ADR-0010's wrapper-arg contract, `#freeze_config_ident` and `#freeze_account_ident` default to `freeze_config` and `freeze_account` when the attr is bare and are overridden by kwargs when the framework's auto-wrap emits resolved names (`#[require_not_frozen(freeze_config = my_cfg, freeze_account = my_frozen, caller = sender)]`).

Distinct variants for gate rejection (`Frozen` / `AccountFrozen`) versus management-op no-op errors (`AlreadyFrozen` / `AccountAlreadyFrozen`). The lenient decoder `from_data_or_default` treats empty bytes as the never-frozen default and errors only on malformed non-empty bytes — the common path for a healthy program stays fast.

## Borsh encoding summary

| Type                 | Size    | Layout                  |
| -------------------- | ------- | ----------------------- |
| `FreezeConfig`       | 33 B    | `[AccountId; 32] ++ u8` |
| `FrozenAccountState` | 1 B     | `u8`                    |

No version byte. No length prefix. Borsh's `try_from_slice` (which `FreezeConfig::decode` uses) rejects trailing bytes on strict decode, so a naive field-append v2 is a wire-breaking change. Version bytes are deferred to a v2 revision if the account model ever needs to evolve.

## Errors

Library-level error enum, mapped to `SpelError::Unauthorized` at the SPEL boundary:

```rust
pub enum FreezeError {
    NotInitialized,             // empty data in freeze_config
    AlreadyInitialized,         // reinit attempt
    DecodingFailed,             // Borsh decode failure on non-empty data
    EncodingFailed,             // Borsh encode failure
    AccountDataTooLarge,        // write_to exceeded the account's data cap
    InvalidCandidate,           // FreezeCandidate validation failed
    UndeployedPda,              // PDA candidate not yet deployed
    CandidateMismatch,          // PDA address doesn't match derivation
    NotFreezeAuthority,         // signer != slot.holder()
    NotAdmin,                   // signer != admin_config.admin
    NotAdminOrFreezeAuthority,  // dual-path auth: neither matched
    MissingSignature,           // is_authorized == false
    Renounced,                  // slot.is_renounced() == true
    AlreadyFrozen,              // freeze_program when is_frozen already true
    NotFrozen,                  // freeze_program_release when is_frozen already false
    AccountAlreadyFrozen,       // freeze_account when target's PDA already true
    AccountNotFrozen,           // freeze_account_release when target's PDA absent or false
    Frozen,                     // gate rejection: program is currently frozen
    AccountFrozen,              // gate rejection: caller's per-account PDA is frozen
}

impl From<FreezeError> for SpelError {
    fn from(e: FreezeError) -> Self {
        SpelError::Unauthorized { message: e.to_string() }
    }
}
```

Every variant maps to `SpelError::Unauthorized` at the SPEL boundary, with the granular reason preserved in the message string. The library-level enum is what tests and handler-side branching consume. Two families of "frozen" variants intentionally coexist: `Frozen` / `AccountFrozen` fire from the auto-wrap prologue when a gated instruction is called on a frozen program or a frozen account, while `AlreadyFrozen` / `AccountAlreadyFrozen` fire when management ops try to no-op transition an already-in-state value. Distinct errors let callers distinguish "your call was blocked" from "you tried to redo a completed transition".

## PDA address summary

| PDA name             | Seed                                          | Owner program        |
| -------------------- | --------------------------------------------- | -------------------- |
| `admin_config`       | `(program_id, "admin_config")`                | `admin-authority`    |
| `freeze_config`      | `(program_id, "freeze_config")`               | `freeze-authority`   |
| Per-account freeze   | `(program_id, "frozen", target_account_id)`   | `freeze-authority`   |

freeze-authority creates the latter two via its management instructions. admin-authority creates `admin_config` independently. The hard dep (ADR-0001) means `freeze_initialize` reads `admin_config` to assert admin signed — but does not write to it.

## See also

- [authority-lifecycle.md](authority-lifecycle.md) — state machines and transition rules over these accounts.
- [ADR-0001 Hard dep on admin-authority](adr/0001-hard-dep-on-admin-authority.md) — why `FreezeConfig` doesn't carry admin fields.
- [ADR-0005 Local FreezeCandidate](adr/0005-local-freeze-candidate.md) — superseded by the spel-authority extraction; `FreezeCandidate` is now `pub type FreezeCandidate = authority::AuthorityCandidate`.
- [ADR-0006 freeze_initialize requires admin signature](adr/0006-freeze-initialize-requires-admin-signature.md) — why `freeze_initialize` reads `admin_config`.
- [ADR-0007 Renounce vacates, not terminal](adr/0007-renounce-vacates-not-terminal.md) — Renounced state semantics.
