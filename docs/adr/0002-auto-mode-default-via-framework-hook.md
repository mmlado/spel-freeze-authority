# Bare `#[freeze_authority]` defaults to auto mode; auto-wrap is implemented via a framework hook

The bare module-level attribute `#[freeze_authority]` enables auto mode (every dispatched instruction is gated by the freeze check, except the exempt set). `#[freeze_authority(manual)]` opts into manual mode where consumers annotate each gated instruction with `#[require_not_frozen]`. The auto-wrap is implemented in `spel-framework-macros` via a new `[package.metadata.spel.wrap_instructions]` metadata field; freeze-authority's macro stays a pass-through marker.

The proposal example (lines 14-44) showed `#[freeze_authority]` as manual and `(auto)` as the explicit opt-in. The defaults are reversed here so the F3-conformant choice is the no-argument form, matching the proposal's own "auto mode is the safer default" claim at the call site. The capability surface is identical; only the default flips.

## Considered Options

**1. Bare = auto, `(manual)` opts in; framework hook in `spel-framework-macros` (chosen).**
F3 conformance is the no-argument choice. The framework hook is a generic primitive that any future extension can reuse. Framework reads the consumer's `#[freeze_authority]` attribute arg during `#[lez_program]` expansion via `mod.attrs` iteration; Cargo metadata declares which arg values skip wrap.
Cost: requires a second upstream PR to `spel-framework-macros` on top of admin-authority's PR #233. Maintainer-review cycle.

**2. Bare = manual, `(auto)` opts in (proposal-faithful).**
Honors the proposal's exact attribute shape.
Rejected because:
- Default is the unsafe choice. Consumer forgets to annotate → F3 leaks. Defeats the "secure by default" framing.
- The proposal's "safer default" narrative becomes false at the call site.
- Manual mode still ships (Q3=A); proposal's manual-as-an-option commitment is preserved.

**3. Two attributes (`#[freeze_authority]` for discovery, `#[freeze_wrap]` for activation).**
Splits discovery and wrap into separate attributes. Schema fully positive.
Rejected because two attributes for one extension is more surface than necessary. Consumers in auto mode would type both; the arg-on-single-attribute pattern is more compact and matches the proposal's shape.

**4. Library-only macro placed above `#[lez_program]`.**
`#[freeze_authority]` is a real attribute macro that walks the consumer mod, prepends `#[require_not_frozen]` to every `#[instruction]` fn unless `#[freeze_exempt]` present, then re-emits a marker for framework discovery.
Rejected because of attribute-ordering: `#[lez_program]` would have to expand AFTER `#[freeze_authority]`, contradicting the proposal example and creating silent-failure risk if order is wrong.

**5. Single mode (auto only).**
Drop manual mode entirely.
Rejected because the proposal commits to two modes (Q3=A). Selective-gating use cases exist.

## Framework hook metadata

```toml
[package.metadata.spel.wrap_instructions]
wrapper = "freeze_authority_macros::require_not_frozen"
skip = ["manual"]
self_exempt_marker = "freeze_exempt"
exempt = [
  "admin_authority::admin_initialize",
  "admin_authority::admin_transfer",
  "admin_authority::admin_renounce",
]
```

- `wrapper`: proc-macro attribute the framework prepends onto each non-exempt dispatched instruction when wrap is active. Same `require_not_frozen` consumers apply by hand in manual mode — one proc-macro, two callers.
- `skip`: arg literals on the consumer's `#[freeze_authority]` attribute that skip wrap. Default behavior (bare attr or any arg not in this list) is wrap. Only `manual` skips for this extension.
- `self_exempt_marker`: attribute name the framework recognizes as "skip this fn from wrap". freeze-authority's own management instructions self-declare via `#[freeze_exempt]`; consumer instructions in auto mode can also use it. Same attribute, double duty.
- `exempt`: cross-crate dispatched instructions to skip. Only contains admin-authority's three because we don't touch admin-authority's source; freeze-authority's own management instructions use the self_exempt_marker instead.

## Consumer-facing API

- `#[freeze_authority]` (bare) — discovery + auto-wrap.
- `#[freeze_authority(manual)]` — discovery only.
- `#[require_not_frozen]` — instruction-level opt-in for manual mode.
- `#[freeze_exempt]` — instruction-level opt-out for auto mode (and self-declares freeze management instructions as exempt internally).

## Consequences

- A second upstream PR to `spel-framework-macros` (after admin-authority's PR #233) introduces the `wrap_instructions` metadata field and the framework-side wrap logic. M1 includes design sign-off; M2 implements; M3 lands the merge.
- If maintainers reject `wrap_instructions` upstream, fall back to Option 4 (library-only macro). Macro internals change; consumer-facing API stays identical. Documented as the fallback path in m1-plan.
- `#[require_not_frozen]` is the per-instruction opt-in under manual mode. The same `require_not_frozen` proc-macro is applied by the framework hook in auto mode — one implementation, two callers.
