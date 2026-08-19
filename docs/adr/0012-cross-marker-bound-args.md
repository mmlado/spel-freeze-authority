---
status: accepted
---

# Cross-marker `bound_args`: extensions read peer extensions' offsets

M2.5's embedded mode lets an extension's config live inside a consumer account at a byte offset ([ADR-0011](0011-embedded-freeze-config.md), and the admin ADR the framework side of the feature). freeze-authority's dual-path `freeze_authority_renounce` reads admin state to authorize the admin arm, which means freeze's instruction body needs to know at what offset admin lives. Admin's offset is declared on the admin marker, not the freeze marker. freeze cannot see it without a cross-extension mechanism.

The framework's existing `[[package.metadata.spel.bound_args]]` binds a fn param to a kwarg on the extension's own marker. This ADR extends the binding to reference kwargs on other extensions' markers using `from = "<other_marker>::<kwarg>"` syntax, so freeze declares:

```toml
[[package.metadata.spel.bound_args]]
arg = "admin_offset"
from = "admin_authority::offset"
default = 0
```

and freeze's `freeze_authority_renounce` gains a trailing `admin_offset: usize` bound-const param the framework fills at the dispatch call site with the value from the module's `#[admin_authority(offset = ...)]` kwarg.

## Grammar

`from` accepts two shapes:

- **Self-marker (existing).** `from = "offset"` binds the arg to the extension's own marker's `offset` kwarg. Unchanged.
- **Cross-marker (new).** `from = "admin_authority::offset"` binds the arg to another marker's kwarg. The path is exactly two segments separated by `::`: the marker's `extension_attr` name, then the kwarg name. No dotted paths, no glob, no aliasing.

`default` remains optional and applies to both shapes. When the referenced marker is absent or the kwarg is missing, `default` is used; without `default`, both cases hard-error.

## Resolution

The framework's marker-parsing pass already collects every `#[<ext>(...)]` attribute on the `#[lez_program]` module and produces `MarkerArgs` records keyed by extension name. Cross-marker `bound_args` resolution reads from that same collection:

1. Parse the `from` value. If it contains `::`, split on the first `::` into `(marker, kwarg)`. Otherwise treat the whole string as a kwarg on the extension's own marker.
2. Look up the referenced marker in the module's collected `MarkerArgs`. Missing → fall back to `default`; no `default` → hard error naming the extension whose bound_arg is unsatisfiable and the marker it references.
3. Look up the kwarg on the found marker. Missing → same fallback / hard-error pair.
4. The resolved value is an integer literal stamped at the dispatch call site, excluded from the IDL. Same substrate as self-marker bound_args. `offset` is the only value-carrying kwarg today.

Resolution runs once per (fn, bound_arg) pair per dispatch site. No runtime cost.

## Failure modes and hard errors

- **Referenced marker not on module.** freeze's Cargo.toml declares `from = "admin_authority::offset" default = 0`; consumer does not put `#[admin_authority]` on their module. Default applies (`0`), no error. If freeze declared no default, hard error: "extension `freeze_authority` bound_arg `admin_offset` requires marker `admin_authority` which is not declared on this module".
- **Referenced marker present but kwarg absent.** Consumer has `#[admin_authority]` bare (no `offset`). Same fallback: default applies, no error; without default, same hard error class ("marker `admin_authority` present but kwarg `offset` not declared").
- **Malformed `from` value.** Not `<segment>` or `<segment>::<segment>`. Hard error at metadata read time. Callers surface as a compile error via `resolve_program_deps`.

Circular refs are structurally impossible: resolution is one-hop, reading a literal kwarg value from another marker's attr on the module. That value does not itself reference another marker, so no cycle can form. Deeper-depends-on-shallower (freeze on admin, quorum on both, etc.) is the natural direction and the only one the grammar supports.

Every failure mode surfaces during macro expansion at the consumer's build. No runtime behaviour is silently degraded.

## Scale and composability

The mechanism composes naturally when a third extension depends on both admin and freeze:

```toml
# quorum-authority/Cargo.toml
[[package.metadata.spel.bound_args]]
arg = "admin_offset"
from = "admin_authority::offset"
default = 0

[[package.metadata.spel.bound_args]]
arg = "freeze_offset"
from = "freeze_authority::offset"
default = 0
```

Each extension declares one bound_arg per cross-marker offset it needs. No inherent coupling grows super-linearly with the number of extensions.

**Design property**: cross-marker `bound_args` form an extension-level dependency graph at the marker level, independent of the crate dep graph. quorum-authority's Cargo.toml does not need to add admin-authority or freeze-authority as Rust dependencies unless it uses their types. The cross-marker ref is a runtime marker contract, not a compile-time crate contract. Framework enforces the referenced marker is present at the consumer's build; the Rust type system enforces nothing about it.

## Aliasing footgun and the caller-decodes contract

Cross-marker resolution passes an offset, not a decoded value. Freeze's instruction body still reads admin state itself:

```rust
let admin = AdminConfig::from_account_at(&admin_config, admin_offset).ok();
FreezeConfig::perform_renounce_at(admin.as_ref(), &mut freeze_config, offset, &caller)?;
```

The lenient `.ok()` keeps the holder arm of the dual path alive when admin state is absent or undecodable. Transfer, which is admin-only, decodes strictly with `?`.

When admin and freeze embed into the same consumer account, both role params substitute to that account. The framework merges them into a single transaction account, listed once in the IDL, enum, and validation with the union of their `mut` and `signer` constraints, and the dispatcher clones the one account into both positions of the precompiled call. The body emits exactly one post-state per unique account id (`post_state_pair`), satisfying the LEZ duplicate-account rule. The params stay separate owned values in the library signature, so no borrow conflict arises in any cell.

This lifts a general contract for extension library methods:

> When an extension's method reads state from a peer extension for an authorization decision, it takes the peer extension's decoded config type by reference (`admin: &AdminConfig`), not the peer's account by reference. The caller decodes at the top of the instruction body and passes the decoded value in.

freeze's `renounce`, `perform_renounce`, `perform_renounce_at` all follow this contract. Any future extension that reads a peer extension's state adopts the same shape.

The same expansion-locality wall moves `freeze_authority_transfer` off its gate. `#[require_admin]` on a library fn expands when the library itself compiles, so it can never receive the consumer's `admin_offset`. The transfer therefore does a strict body check instead: decode admin at `admin_offset`, then `assert_admin(caller)`. Renounce was always body-level (dual-path). `freeze_initialize` keeps its gate, it is dedicated-only and embedded mode skips it at discovery.

## Consequences

- Freeze M2.5 supports the full four-cell mode table (dedicated admin + dedicated freeze, dedicated admin + embedded freeze, embedded admin + dedicated freeze, embedded admin + embedded freeze) including admin and freeze both embedded in the same consumer account.
- Framework side adds one new `bound_args` source (`<marker>::<kwarg>` syntax), one resolution pass reading the module's `MarkerArgs` collection, and four hard-error classes at metadata / resolution time.
- Extension authors gain a stable mechanism for depending on peer extensions' compile-time offsets without inheriting each other's crate deps.
- Framework README documents the mechanism in the embedded mode section. spel-freeze-authority's CONTEXT covers freeze's specific `admin_offset` binding.
- Existing self-marker `bound_args` behaviour is unchanged. `from = "offset"` still means "this extension's own marker's offset kwarg".
- freeze's `renounce` / `perform_renounce` / `perform_renounce_at` API takes `admin: Option<&AdminConfig>` instead of `admin_account: &AccountWithMetadata`, breaking direct callers of the M2 API. Migration is one line: decode admin leniently first (`.ok()`), pass `admin.as_ref()`. `None` fails the admin arm and leaves the holder arm alive. Documented as a lift in freeze's CHANGELOG (if freeze has one) or the M2.5 release notes.

## Rejected alternatives

1. **Reject the dual-embed-same-account case entirely.** Consumers who use both admin and freeze must keep at least one dedicated. Cheaper framework work but consumers lose the "one-account-holds-all-extensions" storage optimization that embedded mode is supposed to enable. Poor advertisement for admin M2.5's headline feature.
2. **Hard-code admin at offset 0.** freeze always reads admin at offset 0 regardless of admin's actual marker. Trivially wrong once admin is embedded at any non-zero offset. Works only for dedicated admin, which defeats the point.
3. **Move the dual-path renounce out of the freeze library.** Consumer writes `freeze_authority_renounce` themselves in embedded mode, knowing both offsets from their own program-wide constants. Framework's `[embedded.skip]` drops the emitted version. Rejected because it forks the freeze instruction set between dedicated and embedded modes, and every consumer writes the same boilerplate.
4. **Cross-marker resolution via a global constants table on the module.** Consumer declares `#[spel_constants(admin_offset = 32, freeze_offset = 65)]` and each extension binds `from = "spel_constants::admin_offset"`. Adds a new attribute for the consumer to maintain in addition to their extension markers, which duplicates values the markers already carry. Rejected as duplication.
5. **Cross-marker resolution via a shared "authority-context" trait.** A trait implemented by each extension exposing `fn offset() -> usize`. Requires a shared crate hosting the trait and Rust trait imports across extensions. Rejected as heavier coupling than the `from = "<marker>::<kwarg>"` string, which is a runtime contract and not a compile-time crate contract.
