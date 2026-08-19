# `FreezeCandidate` is defined locally, not imported from `admin-authority`

**Status:** superseded by spel-authority [ADR-0001](https://github.com/mmlado/spel-authority/blob/main/docs/adr/0001-extract-shared-authority-primitives.md) (extract shared authority primitives). `FreezeCandidate` is now `pub type FreezeCandidate = authority::AuthorityCandidate`, not a local duplicate. Kept below for historical context — option 3's rejection reasoning ("cheap to do later") is exactly what happened.

freeze-authority defines its own `FreezeCandidate` enum with the same shape and semantics as `admin_authority::AdminCandidate`:

```rust
pub enum FreezeCandidate {
    Signer,
    Pda { program_id: AccountId, seed: [u8; 32] },
}
```

The validation method `FreezeCandidate::validate_with_account(account: &AccountWithMetadata) -> Result<AccountId, FreezeError>` mirrors `AdminCandidate::validate_with_account` exactly.

## Considered Options

**1. Local `FreezeCandidate` (chosen).**
~30 lines of duplicated validation logic. IDL for `freeze_authority_transfer` shows `FreezeCandidate` as the parameter type — self-documenting for freeze-only readers. Zero coupling to admin-authority's type evolution.

**2. Reuse `admin_authority::AdminCandidate`.**
Zero duplication. Import and use directly.
Rejected because:
- IDL for `freeze_authority_transfer` would show `AdminCandidate` as a parameter type — confusing for devs reading freeze-authority docs in isolation.
- Couples freeze-authority's API surface to admin-authority's naming choices. If admin-authority later renames or evolves `AdminCandidate`, freeze-authority's IDL is affected.

**3. Generalize upstream to `AuthorityCandidate`.**
Rename `AdminCandidate` → `AuthorityCandidate` in admin-authority. Both libraries reuse.
Rejected because:
- The DRY benefit is small (30 lines). The cross-library coordination cost is larger.
- Can be done later as a separate refactor if both libraries find the duplication painful.

## Consequences

- ~30 lines of duplicated validation logic in `freeze-authority/src/lib.rs`. Acceptable.
- `FreezeError::InvalidCandidate`, `FreezeError::UndeployedPda`, `FreezeError::CandidateMismatch` mirror admin-authority's error variants by name. Hand-mapped, not derived.
- If both libraries' candidate types diverge over time (e.g., admin-authority adds a third variant freeze doesn't need), the divergence is local and contained.
- If a future consolidation is wanted, a shared `authority-candidate` crate can extract both. Cheap to do later; expensive to do now.
