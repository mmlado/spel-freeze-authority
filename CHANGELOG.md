# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `freeze_authority_nominate` and `freeze_authority_accept` instructions.
- `FreezeConfig::nominate`, `accept`, `nominee`, `perform_nominate_at` and `perform_accept_at`.
- `FreezeError::NoNomination`, `NotNominee` and `UnownedSigner`.
- `admin_nominate` and `admin_accept` on the auto-wrap exempt list.
- `FreezeConfig::from_owned_account` and `from_owned_account_at`.
- `FreezeError::code` and `FreezeError::ConfigNotOwned`.

### Changed

- Breaking: `FreezeConfig` encodes as 65 bytes, holder at `0..32`, nominee at `32..64`, `is_frozen` at byte 64.
- Embedded sample: admin at `32..96`, freeze at `96..161`, marker `offset = 96`.
- Breaking: in a program with both libraries, every freeze instruction's discriminant moves up by two, after `admin_nominate` and `admin_accept`. `freeze_authority_nominate` and `freeze_authority_accept` come after `freeze_account_release`.
- `freeze_authority_transfer` and `freeze_authority_renounce` clear the nominee.
- Breaking: `freeze_authority_transfer` and `freeze_authority_accept` refuse a key no program owns.
- Breaking: `FreezeError` maps to `SpelError::Custom` with codes 201 to 224, no longer `SpelError::Unauthorized`.
- Pin `admin-authority` to tag `v0.2.0`, `spel-authority` to tag `v0.2.0` and `spel-framework` to tag `v0.7.0`.

## [0.1.5] - 2026-09-17

### Changed

- `#[instruction]` is re-exported from `spel-framework` rather than shipped as a shim in `freeze-authority-macros`. The framework's own `#[instruction]` strips the `#[account(...)]` helper attrs when it expands outside `#[lez_program]`, so the hand-written copy had nothing left to do (logos-co/spel#271).
- Pin `spel-framework` to `logos-co/spel` at d0bb659, the commit that
  merged that fix.
- Pin `admin-authority` by its `v0.1.4` tag and `spel-authority` by its
  `v0.1.2` tag, the same framework pin in all three.

## [0.1.4] - 2026-09-12

### Changed

- Pin `spel-framework` to upstream `logos-co/spel`, at the commit that
  merged the extension mechanism. The samples move to LEZ `v0.2.4` with
  it.
- Pin `admin-authority` by its `v0.1.3` tag and `spel-authority` by its
  `v0.1.1` tag, the same framework pin in all three.

## [0.1.3] - 2026-09-04

### Fixed

- Reject `#[freeze_authority]` written above `#[lez_program]`. Without the
  guard the marker compiled clean once its name was imported, leaving the
  freeze surface out of the dispatcher with no error.

## [0.1.2] - 2026-08-24

### Changed

- Move `admin-authority` to its `v0.1.2` tag, which aligns the shared
  `spel-authority` pin, one copy of the crate in a dual consumer.

## [0.1.1] - 2026-08-20

### Changed

- The samples' `initialize` fns take their injected params instead of
  declaring them.

## [0.1.0] - 2026-08-19

First release.

### Added

- Seven management instructions covering the authority lifecycle
  (initialize, transfer, renounce) and the freeze surface (program
  freeze and release, per-account freeze and release), with the admin's
  signature required where the lifecycle demands it.
- Auto-wrap gating: the bare `#[freeze_authority]` marker gates every
  dispatched instruction with the dual freeze check.
  `#[freeze_authority(manual)]` switches to per-instruction
  `#[require_not_frozen]`, `#[freeze_exempt]` opts a single instruction
  out, and the instruction that creates an embedded config's account is
  exempted automatically, an account cannot pass a freeze check before
  it exists.
- Embedded mode: `FreezeConfig` can live inside a consumer account,
  sharing that account with the consumer's state and the admin slot.
  The shared account appears once per transaction, freeze operations
  splice only their own byte window, and the admin's location travels
  by a cross-marker bound arg so this crate never depends on where
  admin embedded. The slot is born vacant: the admin appoints the first
  holder through the same transfer path that repopulates a renounced
  slot, so there is no initialization race.
- Verification against a live LEZ node in both modes, including
  post-state pairing, first-touch account claims, and
  release-precedence behavior that dry runs cannot see.
- Three consumer samples, contract tests, an IDL pin test resolving the
  sample's own dependency graph, dry-run byte compares, and `--locked`
  fixture jobs.
- Docs packet: CONTEXT.md vocabulary, account model, authority
  lifecycle, and ADRs.

[Unreleased]: https://github.com/mmlado/spel-freeze-authority/compare/v0.1.5...HEAD
[0.1.5]: https://github.com/mmlado/spel-freeze-authority/releases/tag/v0.1.5
[0.1.4]: https://github.com/mmlado/spel-freeze-authority/releases/tag/v0.1.4
[0.1.3]: https://github.com/mmlado/spel-freeze-authority/releases/tag/v0.1.3
[0.1.2]: https://github.com/mmlado/spel-freeze-authority/releases/tag/v0.1.2
[0.1.1]: https://github.com/mmlado/spel-freeze-authority/releases/tag/v0.1.1
[0.1.0]: https://github.com/mmlado/spel-freeze-authority/releases/tag/v0.1.0
