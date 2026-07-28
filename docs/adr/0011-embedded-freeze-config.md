---
status: accepted
---

# Embedded freeze config: `FreezeConfig` inside a consumer account

The framework's embedded-account mechanism (admin-authority [ADR-0007](https://github.com/mmlado/spel-admin-authority/blob/main/docs/adr/0007-embedded-account-support.md)) lets an extension's config live inside one of the consumer's own accounts at a byte offset instead of a dedicated PDA. freeze-authority adopts it for `FreezeConfig` in the M2.5 release, in lockstep with admin. The consumer opts in on the module marker: `#[freeze_authority(freeze_config = prog_config, offset = N)]` places the 33-byte `FreezeConfig` inside `prog_config`'s data at bytes `[N..N+33)`. Dedicated mode stays the default and remains unchanged, internally it is the degenerate case `offset = 0`, one code path.

## What embed and does not embed

Only `FreezeConfig` is embed-capable. It is the program-wide state, one per program, and a consumer can hold it as a field in their own account type. `FrozenAccountState` stays a per-target PDA at `(program_id, "frozen", target_account_id)`. Its N-per-program shape has no consumer account to relocate it into, and the marker's single embed declaration only knows how to relocate one role. The `freeze_account` inject entry keeps injecting the per-caller PDA as it does today, in both modes. Same shape as admin, where the `caller` signer inject kept its role while `admin_config` moved.

## The extra byte

`FreezeConfig` is 33 bytes on the wire, the 32-byte slot borsh-encoded plus the 1-byte `is_frozen` flag. The framework's shared windowed primitives on `authority::AuthoritySlot` (`read_at` / `write_at`) speak only the 32-byte slot; the flag byte is freeze's business. The library's `_at` variants (`decode_at`, `from_account_at`, `write_to_at`, `bootstrap_at`, `perform_transfer_at`, `perform_renounce_at`, `perform_freeze_at`, `perform_release_at`, `set_is_frozen_at`) splice both spans: the slot via the shared primitive, the flag byte adjacent to it, leaving the surrounding consumer data untouched. Only fixed-size fields may precede the embed offset, a dynamic prefix would make the offset undecidable. Same layout rule as admin.

The consumer holds `FreezeConfig` as a real field in their own account type (`pub freeze: FreezeConfig`), borsh-identical to the underlying slot plus the flag. The `authority` crate stays invisible to consumers.

## Born initialized, no `freeze_initialize`

Embedded mode skips `freeze_initialize`. The consumer's own account-creating instruction (the one carrying `#[account(init)]` on the embedding account) may write the initial state via `FreezeConfig::bootstrap_at`, or may skip it entirely: an all-zeros slot at the declared offset unambiguously means Vacant, and the slot is born vacant until the admin appoints the first holder via `freeze_authority_transfer`, the same path that repopulates a renounced slot. `NotInitialized` keeps its dedicated-mode meaning of "the embedding account does not exist yet". Reinit rejection rides the consumer account's `#[account(init)]`. The admin-side front-running window that ADR-0006 closes for dedicated mode does not exist in embedded mode, because the admin signature is only needed at `freeze_initialize` and that instruction is not emitted.

`[package.metadata.spel.embedded] skip = ["freeze_initialize"]` names the dropped instruction. The instruction stays defined in the library for dedicated-mode consumers; embed mode filters it out at discovery.

Rejected: emitting `freeze_initialize` in embedded mode with a runtime "already exists" check. Duplicates the account creation the consumer already did and hides the born-renounced case behind a second write path.

## Six management instructions take an `offset: usize`

Six management instructions (`freeze_authority_transfer`, `freeze_authority_renounce`, `freeze_program`, `freeze_program_release`, `freeze_account`, `freeze_account_release`) take a trailing `offset: usize` param that the framework fills at the dispatch call site from the marker's `offset` kwarg (or `default = 0` in dedicated mode). The offset is a marker-bound const arg per `[[package.metadata.spel.bound_args]]`, framework-stripped from the IDL. It is never a caller-supplied instruction arg, that would be a caller-controlled write location and every gate would need a runtime bounds check that the framework instead enforces at compile time.

`freeze_program` / `freeze_program_release` need the offset even though they touch only `FreezeConfig`, because they read the slot through `perform_freeze_at` / `perform_release_at`. `freeze_account` / `freeze_account_release` need it for the same reason: their bodies read the current `FreezeConfig` to verify the caller against the freeze authority slot before touching the per-account PDA. Not every management fn writes at the offset, but every one reads through it.

`freeze_initialize` takes no offset. It exists only in dedicated mode (embedded mode skips it at discovery), and it bootstraps a fresh PDA with a whole-account write, so it neither reads nor writes through an offset. The framework's bound-arg pass skips fns that lack the named trailing param, so the non-uniform shape needs no special case.

## Gate keeps working

Auto-wrap in embedded mode substitutes the `freeze_config` role in the discovered management instructions with the consumer's embedding account, name and constraint. The constraint is copied from the consumer's account-creating declaration (`#[account(init, pda = ...)]`), stripped of `init` and `mut`. Consumer-written gates that already declare the embedding account use skip-if-declared; ones that omit it get it injected PDA-verified. Every gate attr the framework prepends is stamped with the location kwargs (`freeze_config = prog_config, offset = N`), so the wrapper macro reads the config at the correct offset. Consumer-authored args disable injection per the existing rule; framework-stamped args do not. The wrapper-arg contract from [ADR-0010](0010-wrapper-args-are-inject-account-names.md) gains one non-role kwarg, `offset`, which is only ever framework-stamped.

The prologue that `require_not_frozen` emits always reads `FreezeConfig::from_account_at(&#freeze_config_ident, offset)`, with `offset` defaulting to `0` when the kwarg is absent. Dedicated mode is the degenerate case, one prologue shape in both modes. The per-account gate (`FrozenAccountState`) reads its own PDA as before, unaffected by the embed.

Consumer-written `freeze_config = ...` or `offset = ...` on `#[require_not_frozen]` is a compile error in embedded mode. It could only contradict the program-wide declaration. The `caller` and `freeze_account` kwargs stay allowed, they remain per-fn concerns and the marker does not speak about them.

## Consequences

- One fewer account on every gated transaction in embedded mode. The freeze slot travels with the consumer's own state.
- Dedicated-mode dry-runs stay byte-identical to M2, since dedicated mode becomes `offset = 0` internally. Existing sample and pin tests do not shift.
- A new sample crate `freeze-authority-sample-embedded/` proves the mode at a non-zero offset, with a neighbor-preservation regression test asserting the consumer's own bytes on both sides of the freeze slot survive a management write.
- A born-vacant regression test proves a consumer that skips the bootstrap gets a slot rejecting every holder-path caller until the admin appoints one via `freeze_authority_transfer`. Not permanent: the appointment path and the renounce-recovery path are the same code.
- `FreezeError::SlotOutOfBounds` variant added, mapped to `SpelError::Unauthorized { message: "embedded slot window out of bounds" }`. Fires only on non-empty data whose layout disagrees with the declared offset. Empty data still yields `NotInitialized`.
- Renounce semantics from ADR-0007 hold in both modes. Admin can repopulate a renounced embedded slot via `freeze_authority_transfer`, which splices the new authority into the same window. The born-renounced case is the same recovery path.
- The authority-suite libraries move in lockstep: admin M2.5 and freeze M2.5 ship as one coordinated release with matching upstream branch heads on `spel-framework`, `spel-authority`, and both extension repos. No cross-version combination ships.
- `FrozenAccountState` is untouched. Per-account frozen state remains an on-demand PDA created by `freeze_account`, indexed by target `AccountId`, with the M2 semantics preserved.

## Rejected alternatives

1. Embed `FrozenAccountState` too, via a marker `freeze_account = <consumer_account>`. Nowhere natural to embed N-per-program state. The consumer would have to allocate one field per potentially-frozen account or a hashmap, and the fixed-offset invariant that makes the framework's stamping deterministic breaks the moment the shape becomes dynamic. Per-account state stays a PDA.
2. Runtime `offset` as an instruction argument. Rejected upstream in admin ADR-0007. A caller-supplied offset is a caller-controlled write location.
3. Whole-data `write_to` against the embedding account. Would overwrite the consumer's neighboring fields. Only splice is safe.
4. Skip `offset` from `freeze_program` / `freeze_program_release` / `freeze_account` / `freeze_account_release` because they do not write the slot. Their bodies still read the slot to verify the caller is the freeze authority; without the offset the read misses.
5. Discriminator byte in the slot (uninit / active / renounced) to disambiguate born-renounced. Same rejection as admin ADR-0007: it diverges from dedicated mode's bare layout and charges every consumer a layout change when switching modes.
