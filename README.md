# spel-freeze-authority

A SPEL library that adds a freeze pattern to LEZ programs: a program-wide frozen flag and a per-account blocklist, both managed by a dedicated freeze authority. Delivered under [RFP-002](https://github.com/logos-co/rfp/blob/main/RFPs/RFP-002-freeze-authority-lib.md), proposal in [logos-co/rfp#47](https://github.com/logos-co/rfp/issues/47).

## What it does

Add `#[freeze_authority]` to a `#[lez_program]` module and the library contributes seven management instructions to the program and gates every other dispatched instruction with a freeze check. The bare attribute is auto mode. `#[freeze_authority(manual)]` disables the automatic gating so the consumer annotates individual instructions with `#[require_not_frozen]` instead. `#[freeze_exempt]` opts a single instruction out of auto mode.

Consumers can name their own params freely. A gated instruction with `#[account(signer)] owner`, `#[account(pda = literal("freeze_config"))] my_cfg`, and `#[account(pda = [literal("frozen"), account("owner")])] my_frozen` reuses all three instead of getting duplicates injected — the framework detects each by role (signer, single-literal PDA, compound-seed PDA) and skips the redundant inject. See [ADR-0010](docs/adr/0010-wrapper-args-are-inject-account-names.md).

The freeze authority lifecycle is governed by the admin from [spel-admin-authority](https://github.com/mmlado/spel-admin-authority) (RFP-001). The dependency is hard: `freeze_initialize` requires the admin's signature. Consumer programs must list both `freeze-authority` and `admin-authority` as direct dependencies. The framework discovers extensions in direct dependencies only, never transitively.

## Workspace

- `freeze-authority` is the library crate with the seven management instructions and the on-chain state types.
- `freeze-authority-macros` holds the proc-macro attributes.
- `freeze-authority-sample` is a consumer program using auto mode.
- `freeze-authority-sample-manual` is a consumer program using manual mode.

## Documentation

- [CONTEXT.md](CONTEXT.md) defines the vocabulary and the instruction surface.
- [docs/account-model.md](docs/account-model.md) describes the on-chain accounts and encodings.
- [docs/authority-lifecycle.md](docs/authority-lifecycle.md) describes the state machines and transitions.
- [docs/adr/](docs/adr/) records the design decisions and their deviations from the proposal.
- [docs/dry-run-output.txt](docs/dry-run-output.txt) is a captured CLI dry-run across the auto-gated consumer instruction and every freeze management instruction. Regenerate with `scripts/dry-run.sh` after any change to the sample or the framework.

The framework-side extension mechanism (discovery, injection, auto-wrap) lives in the [spel fork](https://github.com/mmlado/spel) on the `feat/wrap_instructions` branch.

## Dependencies

The three cross-repo dependencies are pinned to exact revs so the review surface is reproducible from git state alone. Current baseline:

| Dep | Repo | Branch | Rev |
| --- | --- | --- | --- |
| `spel-framework` | [mmlado/spel](https://github.com/mmlado/spel) | `feat/wrap_instructions_m2` | `c701b78` |
| `admin-authority` | [mmlado/spel-admin-authority](https://github.com/mmlado/spel-admin-authority) | `m2_freeze_m2` | `d92f63d` |
| `authority` (`spel-authority`) | [mmlado/spel-authority](https://github.com/mmlado/spel-authority) | `freeze_m2` | `b106e00` |

Bumping any of these requires updating the `rev` field in all Cargo.toml files that reference the dep (`freeze-authority/`, `freeze-authority-sample/`, `freeze-authority-sample-manual/`) plus this table.
