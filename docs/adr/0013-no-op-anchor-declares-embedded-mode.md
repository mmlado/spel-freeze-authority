---
status: accepted
---

# A no-op anchor declares embedded mode

The framework infers embedded mode from an anchor fn when the extension declares `anchor_attr` and `anchor_role` in its embedded metadata (admin-authority [ADR-0013](https://github.com/mmlado/spel-admin-authority/blob/main/docs/adr/0013-embedded-mode-inferred-from-the-anchor-fn.md)). Admin's anchor is a bootstrap: `#[admin_initialize]` injects the slot's first write. Freeze has no bootstrap by design, the slot is born vacant and the admin appoints the first holder via transfer (ADR-0011). That left freeze on the marker kwarg as its only embedded declaration, and left a gap: a consumer could mark `#[freeze_slot]` on a field, forget the kwarg, and silently compile a dedicated-mode program with a dead slot field.

## Decision

freeze-authority declares the anchor pair, `anchor_attr = "freeze_initialize"` and `anchor_role = "freeze_config"`, and ships `#[freeze_initialize]` as a pass-through proc macro that expands to nothing. The attribute declares, it does not inject. The consumer marks the instruction that creates the embedding account, the framework infers embedded mode from it, and the slot is born vacant exactly as ADR-0011 specifies. Nothing about the runtime model changes.

What changes is the consumer surface. The marker is bare `#[freeze_authority]` in both modes, and the role kwarg is retired: writing it on an anchored extension is a hard error in the framework. The embedded declaration is now the same three-part shape as admin's, `#[freeze_slot]` on the field, `#[freeze_initialize]` on the creating instruction, a bare marker on the module.

The agreement between the declarations is hard both ways. A marked field with no anchored fn refuses to build, the field declares embedded mode and nothing anchors it, so the program would compile as dedicated mode with a dead slot window. An anchored fn with no marked field refuses too, the derivation has no carrier. The first case is the gap this ADR closes, before the anchor it compiled silently.

Two anchors on one fn compose. The embedded sample's `initialize` carries `#[admin_initialize]` and `#[freeze_initialize]`, each extension resolves its own embed from the same `#[account(init)]` param, and the `missing_freeze_initialize` compile-fail fixture pins the dormant case from the vacant-slot side.

## Consequences

The dormant marked field moves from silent drift to refusal. Before the anchor, `#[freeze_slot]` with no kwarg produced a working dedicated-mode program with a dead offset const. Now it is a compile error naming the struct, both attributes, and both fixes.

The framework's dormant-anchor message speaks in mode-disagreement terms rather than born-renounced terms, because a vacant slot ships vacant either way. That generalization landed with this adoption.

`freeze_initialize` the attribute shares its name with `freeze_initialize` the dedicated-mode instruction, the same pairing admin uses. Embedded mode drops the instruction via `embedded.skip` and reads the attribute from the consumer's fn.

## Considered alternatives

**Keep the marker kwarg.** No new attribute, no inference. Rejected: it leaves the dormant marked field compiling silently, and it splits the consumer surface in two shapes for no reason once the framework infers anchors.

**Infer embedded mode from the slot field marker alone.** No anchor attribute at all. Rejected: the field marker binds a struct, not an instruction account param, and accounts are untyped at the instruction surface, so there is no static link from the marked struct to the account the instructions receive. The anchor fn's `#[account(init)]` param is that link.

**A bootstrap-style anchor that writes the vacant state.** Symmetry with admin. Rejected: ADR-0011 already rejected a second write path, an all-zeros window at the offset unambiguously means Vacant and the consumer's own write covers it.
