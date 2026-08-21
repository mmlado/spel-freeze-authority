# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/mmlado/spel-freeze-authority/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/mmlado/spel-freeze-authority/releases/tag/v0.1.1
[0.1.0]: https://github.com/mmlado/spel-freeze-authority/releases/tag/v0.1.0
