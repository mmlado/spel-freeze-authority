# Account Model

What freeze-authority stores on-chain. Two account types, each created via SPEL's `#[account_type]` macro and serialized with Borsh.

For state transitions and lifecycle rules, see [authority-lifecycle.md](authority-lifecycle.md). This doc only describes the static shapes.

## `FreezeConfig`

One per program. Stores the freeze authority slot and the program-wide frozen flag.

```rust
#[account_type]
#[derive(BorshSerialize, BorshDeserialize)]
pub struct FreezeConfig {
    pub freeze_authority: AccountId,
    pub is_frozen: bool,
}
```

**PDA derivation:** `(program_id, "freeze_config")`. Single-seed PDA. Address is deterministic per program — no per-instance instances. Reinit rejected after first `freeze_initialize` call per LEZ's `validate_execution` rule.

**Encoding:** fixed 33 bytes — 32-byte `AccountId` + 1-byte bool. Borsh layout is direct: no length prefix, no discriminator, no padding. The fixed encoding lets the framework hook reason about tx size analytically.

**Sentinel for renounced:** `freeze_authority == AccountId::default()` (all zeros) means the slot is Renounced. Pattern-aligned with `admin-authority`'s `admin` field. Per ADR-0007, Renounced is recoverable by admin via `freeze_authority_transfer`.

### State detection

| State         | `account.data` | `freeze_authority`     |
| ------------- | -------------- | ---------------------- |
| Uninitialized | empty          | n/a (decode fails)     |
| Initialized   | non-empty      | non-default            |
| Renounced     | non-empty      | `AccountId::default()` |

Discrimination order in `FreezeConfig::from_account`:

1. If `account.data.is_empty()` → `FreezeError::NotInitialized`.
2. If decode succeeds and `freeze_authority == AccountId::default()` → state is Renounced. Caller chooses how to handle (`assert_freeze_authority` returns `FreezeError::Renounced`; transfer accepts and overwrites).
3. Otherwise compare `signer.account_id` to `freeze_authority` for authorization.

## `FrozenAccountState`

One per frozen `AccountId`. Stores the per-account frozen flag.

```rust
#[account_type]
#[derive(BorshSerialize, BorshDeserialize)]
pub struct FrozenAccountState {
    pub is_frozen: bool,
}
```

**PDA derivation:** `(program_id, "frozen", target_account_id)`. Multi-seed PDA. SPEL's `#[account(pda = [literal("frozen"), account("target")])]` syntax derives the address at runtime from another param (the target). Supported by `spel-framework-macros` since the multi-seed PDA work (per `spel-framework-macros/src/lib.rs:711-714`).

**Encoding:** fixed 1 byte — single bool. Smallest possible per-account state. ADR-0008 (existence-only encoding) was considered for further savings but rejected — proposal commits to bool-inside.

**No sentinel; absence is the default state.** Per ADR-0008: if the PDA doesn't exist on-chain (target was never frozen), the auto-wrap macro treats the missing PDA as `is_frozen = false`. Same treatment as a present PDA storing `false` (a previously-frozen account that was released).

### State detection

| State        | PDA presence | `is_frozen` value |
| ------------ | ------------ | ----------------- |
| Never frozen | absent       | n/a               |
| Frozen       | present      | `true`            |
| Released     | present      | `false`           |

The auto-wrap gate prologue:

```rust
let __cfg = ::freeze_authority::FreezeConfig::from_account(&freeze_config)?;
if __cfg.is_frozen { return Err(FreezeError::AlreadyFrozen.into()); }

if !__frozen_pda.account.data.is_empty() {
    let __fa = <FrozenAccountState as BorshDeserialize>::try_from_slice(&__frozen_pda.account.data)?;
    if __fa.is_frozen { return Err(FreezeError::AccountAlreadyFrozen.into()); }
}
```

Empty data branch passes silently — the most common path for a healthy program (most signers are never frozen). Decoding only runs when the PDA actually exists.

## Borsh encoding summary

| Type                 | Size    | Layout                  |
| -------------------- | ------- | ----------------------- |
| `FreezeConfig`       | 33 B    | `[AccountId; 32] ++ u8` |
| `FrozenAccountState` | 1 B     | `u8`                    |

No version byte. No length prefix. Forward-compatibility via field append is technically possible (Borsh accepts trailing bytes on decode? — verify in M2; if not, version bytes are a v2 concern).

## Errors

Library-level error enum, mapped to `SpelError::Unauthorized` at the SPEL boundary:

```rust
pub enum FreezeError {
    NotInitialized,          // empty data in freeze_config
    AlreadyInitialized,      // reinit attempt
    DecodingFailed,
    EncodingFailed,
    AccountDataTooLarge,
    InvalidCandidate,        // FreezeCandidate validation failed
    UndeployedPda,           // PDA candidate not yet deployed
    CandidateMismatch,       // PDA address doesn't match derivation
    NotFreezeAuthority,      // signer != freeze_config.freeze_authority
    NotAdmin,                // signer != admin_config.admin (for admin-only paths)
    MissingSignature,        // is_authorized == false
    Renounced,               // freeze_authority slot is vacant
    AlreadyFrozen,           // freeze_program when is_frozen already true
    NotFrozen,               // freeze_program_release when is_frozen already false
    AccountAlreadyFrozen,    // freeze_account when target's PDA already true
    AccountNotFrozen,        // freeze_account_release when target's PDA absent or false
}

impl From<FreezeError> for SpelError {
    fn from(_err: FreezeError) -> Self {
        SpelError::Unauthorized
    }
}
```

Mapping is uniform (`Unauthorized`) at the SPEL boundary so consumer error handling stays simple. The library-level enum carries the granular reason for tests and for handler-side branching.

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
- [ADR-0005 Local FreezeCandidate](adr/0005-local-freeze-candidate.md) — `FreezeCandidate` type used to validate new authorities.
- [ADR-0006 freeze_initialize requires admin signature](adr/0006-freeze-initialize-requires-admin-signature.md) — why `freeze_initialize` reads `admin_config`.
- [ADR-0007 Renounce vacates, not terminal](adr/0007-renounce-vacates-not-terminal.md) — Renounced state semantics.
