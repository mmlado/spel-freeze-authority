---
status: accepted
---

# Wrapper macros accept every inject-account name as a keyword arg

Reviewer feedback on M2 flagged that the per-account freeze gate binds by a hardcoded param name (`caller`) rather than by role. A consumer whose signer is named `sender` gets a second `caller` injected next to `sender`, and the frozen PDA derives from the injected placeholder while the real actor goes free. The gate silently no-ops. Same class of problem applies to `freeze_config`: if the consumer already declares an `#[account(pda = literal("freeze_config"))] my_cfg`, injection adds a duplicate `freeze_config` and the wrapper prologue reads the duplicate instead of the consumer's own param.

The fix has two moving parts. The framework needs to detect existing role matches and skip injection for those. The wrapper macro needs to learn the resolved param names so its prologue references them instead of hardcoded defaults. This ADR nails down how those two sides talk to each other.

## Decisions

**Inject-account `name` is the wrapper's arg key.** freeze-authority's Cargo.toml declares three inject accounts under `require_not_frozen`: `freeze_config`, `freeze_account`, `caller`. The wrapper macro accepts kwargs by those same names. No mapping layer, no schema field, one namespace. `#[require_not_frozen(freeze_config = my_cfg, freeze_account = my_frozen, caller = sender)]` is the canonical shape.

**Framework auto-wrap emits every inject-account name.** When the framework applies a wrap to a consumer instruction, it emits every account from the matching inject spec as a kwarg with the resolved name (post role-remap). If nothing was remapped, resolved names equal declaration names and the emitted attribute reads `#[require_not_frozen(freeze_config = freeze_config, freeze_account = freeze_account, caller = caller)]`, which behaves identically to the bare form. Both bare and args forms activate injection so the framework's own auto-wrap path is closed and manual consumers pick whichever shape suits their taste.

**Wrapper macros hard-error on unknown keys.** The accepted set is closed. A key the wrapper doesn't recognise is a compile error naming the offending key. This forces the extension author to keep the wrapper's accepted set in sync with the Cargo.toml inject spec instead of drifting quietly.

**Alignment is the extension author's responsibility, verified by a self-test.** Each extension ships a unit test that parses its own Cargo.toml, iterates the declared inject-account names, and invokes the wrapper with each as a kwarg. Any "unknown key" error fails the test. Drift is caught in the extension's own CI before a consumer ever sees the compile break.

**Role-based reuse spans signer, single-literal PDA, and compound-seed PDAs, embedded roles included.** An embedded role's rewritten inject entry carries the canonical constraint copied from the consumer's account-creating declaration, and flows through the same reuse machinery: recognition is by seed spec, so a consumer declaring the embedding account under any name reuses it, and a same-name declaration with a different constraint is a compile error. During injection, if an inject account is marked `signer = true` and the fn already carries a `#[account(signer)]` param, skip the inject and remap seed refs to the existing param. Same for a single `Const` seed: an existing `#[account(pda = literal("<seed>"))]` param wins. Compound seeds like `[literal("frozen"), account("caller")]` also reuse: `build_remap` runs in two phases so that the compound-seed comparison sees the signer and single-literal remaps already resolved. A consumer's `#[account(pda = [literal("frozen"), account("sender")])] my_frozen` next to `#[account(signer)] sender` remaps `freeze_account` to `my_frozen` because the compound comparison resolves `account("caller")` to `account("sender")` via the phase-1 signer remap.

## Consequences

- `require_not_frozen` accepted keys rename from `config` to `freeze_config`. `require_admin` renames from `config` to `admin_config` and from `signer` to `caller`. Both add explicit accepts for any inject accounts they don't reference (e.g. `require_not_frozen` accepts `caller` even though its prologue doesn't read it).
- Consumers writing `#[account(signer)] sender` on an auto-gated instruction get the freeze gate working correctly against `sender`, not against a phantom `caller`. Same for consumers writing their own `#[account(pda = literal("freeze_config"))] my_cfg`.
- The framework schema does not gain a `macro_arg` field. Inject specs stay flat: `name`, `seed`, `signer`.
- The framework's `resolve_program_deps` call chain does not need to know the wrapper's parser. Alignment is the extension's problem and stays inside the extension repo.
- The self-test needs `read_spel_inject_specs` (or a thin path-taking wrapper) exposed from `spel-framework-core::extension`. One-line pub change.
- Consumers writing wrapper attrs by hand with only some args (`#[require_not_frozen(freeze_config = my_cfg)]`) still work: the wrapper's other accepted keys fall back to their defaults inside the wrapper macro. Framework's own auto-wrap always emits the full set to eliminate any doubt about which name won.

## Rejected alternatives

1. **`macro_arg` field on `InjectAccount`.** Framework only emits declared args, wrappers only accept the ones actually referenced. Two namespaces (inject-spec name vs macro-arg key) with a mapping between them, plus schema churn on both the framework side and every extension's Cargo.toml. Nothing gained that a shared naming convention doesn't already give.
2. **Wrapper macros walk `fn.sig.inputs` themselves to find role matches.** Removes the arg-emission step entirely: wrapper detects the signer or the freeze_config PDA on its own. Duplicates the same role-detection logic between framework and wrapper, and every wrapper reinvents it. Worse when a third extension wants to reuse the primitive.
3. **Wrapper macros silently ignore unknown keys.** Aligns better with extensibility (adding a new inject account never breaks anything), but drift between Cargo.toml and wrapper becomes invisible. The `caller` unused-arg case reads correctly by accident until someone renames the account and the wrong param starts appearing in prologues without warning. A hard error surfaces the mismatch immediately.
4. **Framework-level check of alignment.** A helper that walks every extension's Cargo.toml at framework build time and validates. Framework does not know which crates are extensions until a consumer wires them in, so this check has nowhere to live.
