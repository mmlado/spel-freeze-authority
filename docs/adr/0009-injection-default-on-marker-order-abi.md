---
status: accepted
---

# Injection is always on behind a hard error, marker order is the cross-extension ABI

admin-authority's ADR-0006 left the release posture of param injection open for maintainer feedback, and the old implementation left cross-extension ordering to fall out of the dependency scan. Both are now decided, together with the collision rule the ordering question implied.

## Decisions

**Injection is always on, no debug/release split.** The module marker is already an explicit consumer opt-in, and skip-if-declared keeps the explicit style available, so a gated variant would only add a mode. The precondition the reviewer set is met: malformed inject metadata is a hard compile error. A bare-string seed, a non-string wrapper, or an unknown seed entry aborts the build instead of degrading to an unconstrained param where a PDA-verified one was intended. Behaviour never differs between debug and release.

**Only a bare wrapper attr activates injection.** A wrapper attr carrying arguments, `#[require_admin(config = my_cfg, signer = owner)]`, names custom target params. Whoever renames also declares: injection skips that instruction entirely, so the spec's default names never dangle next to the custom ones.

**Marker order on the module is the cross-extension ABI.** When two extensions contribute to one program, the order of their marker attrs on the `#[lez_program]` module decides everything downstream: instruction order in the dispatcher and IDL, and the account indices of injected params. First marker contributes first. Injected params append across specs with one running cursor, after a leading `ProgramContext`. The order is consumer-visible and self-documenting, and reordering markers is an explicit ABI change in the consumer's own source.

**Same-name collisions dedup or fail.** When two extensions inject the same param name with identical constraints, they share one account at the first injector's position. That is the cheap shared-signer ABI: one `caller` serves both gates for +32 bytes instead of a second dedicated signer. Conflicting constraints are a compile error naming both extensions, because whichever constraint won, the other gate would read a wrong account and fail at runtime with no hint that two libraries are incompatible.

## Consequences

- A consumer can rely on injected params having stable indices as long as the marker order in their module is stable. Reordering markers reshuffles account indices and is visible in their own diff.
- Renaming an extension crate no longer changes the ABI. The old behaviour ordered specs alphabetically by crate name through the dependency scan.
- Extensions sharing a signer param by convention (same name, `signer = true`, no seed) compose for free.
- A consumer who wants custom param names gives up injection for that instruction and declares everything, which the gate supports through its attribute arguments.

## Rejected alternatives

1. Release builds require explicit declaration (`#[cfg(not(debug_assertions))] compile_error!`). Prototyped and works, but `debug_assertions` tracks the opt profile rather than a real production flag, and behaviour differing between profiles is its own failure class.
2. Wrapper-attr order on the instruction as the ABI. Less visible than the module head, and repeats per instruction with room to disagree.
3. Alphabetical-by-crate-name, documented. Deterministic but semantically arbitrary, and a crate rename silently reshuffles account indices.
4. Erroring on identical-constraint collisions. The consumer controls neither extension's param names, so the error would make two extensions unusable together for no semantic gain.
