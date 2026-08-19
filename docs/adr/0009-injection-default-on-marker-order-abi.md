---
status: accepted
---

# Injection is always on behind a hard error, marker order is the cross-extension ABI

admin-authority's ADR-0006 left the release posture of param injection open for maintainer feedback, and the old implementation left cross-extension ordering to fall out of the dependency scan. Both are now decided, together with the collision rule the ordering question implied.

## Decisions

**Injection is always on, no debug/release split.** The module marker is already an explicit consumer opt-in, and skip-if-declared keeps the explicit style available, so a gated variant would only add a mode. The precondition the reviewer set is met: malformed inject metadata is a hard compile error. A bare-string seed, a non-string wrapper, or an unknown seed entry aborts the build instead of degrading to an unconstrained param where a PDA-verified one was intended. Behaviour never differs between debug and release.

**Bare and args forms of the wrapper attr both activate injection.** [ADR-0010](0010-wrapper-args-are-inject-account-names.md) settled the wrapper-arg contract: the framework's own auto-wrap emits kwargs on every wrap attr so the wrapper receives resolved names, which means args-form has to activate. Role-based remap covers the rename case a "bare-only" rule would have guarded, so a consumer who names a signer `owner` or a PDA `my_cfg` still gets injection to skip those params via role detection regardless of whether the wrapper attr is bare or carries arguments.

**Marker order on the module is the cross-extension ABI.** When two extensions contribute to one program, the order of their marker attrs on the `#[lez_program]` module decides everything downstream: instruction order in the dispatcher and IDL, and the account indices of injected params. First marker contributes first. Injected params append across specs with one running cursor, after a leading `ProgramContext`. The order is consumer-visible and self-documenting, and reordering markers is an explicit ABI change in the consumer's own source.

**Same-name collisions dedup or fail.** When two extensions inject the same param name with identical constraints, they share one account at the first injector's position. That is the cheap shared-signer ABI: one `caller` serves both gates for +32 bytes instead of a second dedicated signer. Conflicting constraints are a compile error naming both extensions, because whichever constraint won, the other gate would read a wrong account and fail at runtime with no hint that two libraries are incompatible.

## Consequences

- A consumer can rely on injected params having stable indices as long as the marker order in their module is stable. Reordering markers reshuffles account indices and is visible in their own diff.
- Renaming an extension crate no longer changes the ABI. The old behaviour ordered specs alphabetically by crate name through the dependency scan.
- Extensions sharing a signer param by convention (same name, `signer = true`, no seed) compose for free.
- Consumers who want custom param names declare them with the usual `#[account(...)]` attrs. Role-based remap detects the signer and the PDA-literal params and skips those injections. The wrapper attr can stay bare or carry the resolved names as kwargs; either shape triggers activation.

## Rejected alternatives

1. Release builds require explicit declaration (`#[cfg(not(debug_assertions))] compile_error!`). Prototyped and works, but `debug_assertions` tracks the opt profile rather than a real production flag, and behaviour differing between profiles is its own failure class.
2. Wrapper-attr order on the instruction as the ABI. Less visible than the module head, and repeats per instruction with room to disagree.
3. Alphabetical-by-crate-name, documented. Deterministic but semantically arbitrary, and a crate rename silently reshuffles account indices.
4. Erroring on identical-constraint collisions. The consumer controls neither extension's param names, so the error would make two extensions unusable together for no semantic gain.
5. Bare wrapper attrs activate, args-form disables injection. Prototyped and initially accepted; retracted before merge because the framework's own auto-wrap needs to emit resolved names on the wrapper attr, so args-form must activate. Role-based remap replaces the "hands off on args" semantic without losing rename support.
