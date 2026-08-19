# spel-freeze-authority

[![CI](https://github.com/mmlado/spel-freeze-authority/actions/workflows/ci.yml/badge.svg)](https://github.com/mmlado/spel-freeze-authority/actions/workflows/ci.yml)
[![Consumer build](https://github.com/mmlado/spel-freeze-authority/actions/workflows/consumer-build.yml/badge.svg)](https://github.com/mmlado/spel-freeze-authority/actions/workflows/consumer-build.yml)
[![Version](https://img.shields.io/github/v/tag/mmlado/spel-freeze-authority?label=version)](https://github.com/mmlado/spel-freeze-authority/releases)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE-MIT)
[![MSRV](https://img.shields.io/badge/MSRV-1.88-93450a?logo=rust)](Cargo.toml)

A SPEL library that adds a freeze pattern to LEZ programs: a program-wide frozen flag and a per-account blocklist, both managed by a dedicated freeze authority. Delivered under [RFP-002](https://github.com/logos-co/rfp/blob/main/RFPs/RFP-002-freeze-authority-lib.md), proposal in [logos-co/rfp#47](https://github.com/logos-co/rfp/issues/47).

## What it does

Add `#[freeze_authority]` to a `#[lez_program]` module and the library contributes seven management instructions to the program and gates every other dispatched instruction with a freeze check. The bare attribute is auto mode. `#[freeze_authority(manual)]` disables the automatic gating so the consumer annotates individual instructions with `#[require_not_frozen]` instead. `#[freeze_exempt]` opts a single instruction out of auto mode. One exemption is automatic: in embedded mode the instruction that creates the embedding account with `#[account(init)]` is never auto-gated, an account cannot pass a freeze check before it exists. Manual mode is unchanged, an authored `#[require_not_frozen]` stays where the author put it.

Consumers can name their own params freely. A gated instruction with `#[account(signer)] owner`, `#[account(pda = literal("freeze_config"))] my_cfg`, and `#[account(pda = [literal("frozen"), account("owner")])] my_frozen` reuses all three instead of getting duplicates injected — the framework detects each by role (signer, single-literal PDA, compound-seed PDA) and skips the redundant inject. See [ADR-0010](docs/adr/0010-wrapper-args-are-inject-account-names.md).

The freeze authority lifecycle is governed by the admin from [spel-admin-authority](https://github.com/mmlado/spel-admin-authority) (RFP-001). The dependency is hard: `freeze_initialize` requires the admin's signature. Consumer programs must list both `freeze-authority` and `admin-authority` as direct dependencies. The framework discovers extensions in direct dependencies only, never transitively.

Since M2.5 the freeze state can live inside one of the consumer's own accounts instead of a dedicated PDA, sharing that account with the consumer's state and the admin slot. See the embedded mode section below.

## Embedded mode

Declared program-wide on the marker, in lockstep with admin-authority's embedded mode:

```rust
#[account_type]
pub struct ProgramConfig {
    pub value: u64,            // bytes 0..8
    pub padding: [u8; 24],     // bytes 8..32
    #[admin_slot]
    pub admin: AdminConfig,    // bytes 32..64
    #[freeze_slot]
    pub freeze: FreezeConfig,  // bytes 64..97
}

#[lez_program]
#[admin_authority(admin_config = config, offset = 32)]
#[freeze_authority(freeze_config = config, offset = 64)]
mod my_program { ... }
```

What changes versus dedicated mode:

- **No `freeze_initialize`.** The slot is born vacant: the consumer's account-creating instruction writes the struct and the admin appoints the first holder via `freeze_authority_transfer`, the same path that repopulates a renounced slot. There is no front-running window because there is no initializer to race. The admin slot next door is bootstrapped by marking that same instruction with `#[admin_initialize]` from the admin-authority crate.
- **Slot markers keep the layout honest.** `#[admin_slot]` and `#[freeze_slot]` each derive an offset const and a layout test, and the build fails if a marker position and its declared `offset` disagree.
- **One account per transaction.** When admin and freeze share the embedding account, the framework merges the two role params into one transaction account and the library emits exactly one post-state for it. The IDL lists the shared account once.
- **Admin's location travels by marker.** `freeze_authority_transfer` and `freeze_authority_renounce` read admin state at the offset declared on the admin marker. The framework resolves it at the consumer's build (a cross-marker bound arg, [ADR-0012](docs/adr/0012-cross-marker-bound-args.md)) and bakes it into the dispatcher as a literal.
- **No offset is ever in a transaction.** All offsets compile into the program. Changing one changes the bytecode, which on LEZ is a different program.
- **Dedicated mode is untouched.** Internally it is the degenerate case offset 0, and the dedicated dry-run remains byte-identical to the M2 pin.

Design records: [ADR-0011](docs/adr/0011-embedded-freeze-config.md) (embedded freeze config), [ADR-0012](docs/adr/0012-cross-marker-bound-args.md) (cross-marker bound args). Walkthrough: `scripts/dry-run-embedded.sh`, expected output in [docs/dry-run-embedded-output.txt](docs/dry-run-embedded-output.txt).

## Transaction size overhead

Measured from the committed dry-run captures ([docs/dry-run-output.txt](docs/dry-run-output.txt), [docs/dry-run-embedded-output.txt](docs/dry-run-embedded-output.txt)).

Dedicated mode: the auto-wrap gate adds two accounts to a gated transaction, the `freeze_config` PDA (32 byte id in the message, 33 byte pre-state in the witness) and the caller's per-account `freeze_account` PDA (32 byte id, 1 byte pre-state once created). In the captures the gated `update_value` carries 4 accounts while the exempt `read_value` carries 1.

Embedded mode: `freeze_config` rides inside the consumer's own account, so the gate adds one account (`freeze_account`) and the freeze state adds 33 bytes to the embedding account's data. Management instructions that read admin state also drop the dedicated `admin_config` account when admin embeds in the same account: `freeze_authority_renounce` carries 3 accounts in dedicated mode and 2 in embedded mode.

Instruction data is unchanged by the gates in both modes. The 132 byte data on `freeze_account` and `freeze_account_release` is the `target` argument encoding (risc0 serde encodes `[u8; 32]` as 32 words), unrelated to the gate.

## Workspace

- `freeze-authority` is the library crate with the seven management instructions and the on-chain state types.
- `freeze-authority-macros` holds the proc-macro attributes.
- `freeze-authority-sample` is a consumer program using auto mode.
- `freeze-authority-sample-manual` is a consumer program using manual mode.
- `freeze-authority-sample-embedded` is a consumer program with admin and freeze state embedded in one shared account.

## Documentation

- [CONTEXT.md](CONTEXT.md) defines the vocabulary and the instruction surface.
- [docs/account-model.md](docs/account-model.md) describes the on-chain accounts and encodings.
- [docs/authority-lifecycle.md](docs/authority-lifecycle.md) describes the state machines and transitions.
- [docs/adr/](docs/adr/) records the design decisions and their deviations from the proposal.
- [docs/dry-run-output.txt](docs/dry-run-output.txt) is a captured CLI dry-run across the auto-gated consumer instruction and every freeze management instruction. Regenerate with `scripts/dry-run.sh` after any change to the sample or the framework.
- [docs/dry-run-embedded-output.txt](docs/dry-run-embedded-output.txt) is the same capture for the embedded sample, showing the shared account appearing once per transaction. Regenerate with `scripts/dry-run-embedded.sh`.

The framework-side extension mechanism (discovery, injection, auto-wrap, cross-marker bound args, the shared-account merge) lives on the [spel fork](https://github.com/mmlado/spel)'s main, upstreaming via [logos-co/spel#257](https://github.com/logos-co/spel/pull/257).

## Dependencies

The library and admin dependencies pin release tags, the framework pins an exact rev until it has an upstream release. The committed Cargo.lock resolves every pin to a concrete commit either way. Current baseline:

| Dep | Repo | Pin |
| --- | --- | --- |
| `spel-framework` | [mmlado/spel](https://github.com/mmlado/spel) | rev `f7aa464` (v0.6.0 plus the extension mechanism) |
| `admin-authority` | [mmlado/spel-admin-authority](https://github.com/mmlado/spel-admin-authority) | tag `v0.1.0` |
| `authority` (`spel-authority`) | [mmlado/spel-authority](https://github.com/mmlado/spel-authority) | tag `v0.1.0` |

Bumping any of these requires updating the pin in all Cargo.toml files that reference the dep (`freeze-authority/`, `freeze-authority-sample/`, `freeze-authority-sample-manual/`, `freeze-authority-sample-embedded/`) plus this table.

## Versioning and stability

Semantic versioning covers five surfaces: the public Rust API, the attribute names (`#[freeze_authority]`, `#[freeze_initialize]`, `#[freeze_slot]`, `#[require_not_frozen]`, `#[freeze_exempt]`) with their argument grammar, the `[package.metadata.spel]` contract this crate declares including the wrap and bound-arg entries, `FreezeConfig`'s 33-byte borsh encoding, an on-chain wire format, and the cross-marker offset contract peers read admin state through. While the version is 0.x, a minor bump may change any of them, and each such change is called out in the changelog.

The `spel-framework` dependency pins a fork revision for now. Version 1.0.0 lands when the extension mechanism reaches an upstream release ([logos-co/spel#257](https://github.com/logos-co/spel/pull/257)) and the pin moves to it.

## Support

Clarification questions and in-scope fixes are covered for 90 days after milestone approval. Platform upgrades past the pinned LEZ rev are new scope.

## License

Dual-licensed under [MIT](LICENSE-MIT) and [Apache 2.0](LICENSE-APACHE2) at the consumer's option.
