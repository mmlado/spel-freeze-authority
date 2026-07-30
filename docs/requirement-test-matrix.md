# Requirement-to-test matrix, RFP-002 (M3)

Every hard requirement and every proposal-enumerated scenario, with the artifact that satisfies it. Proposal instruction names map to the shipped names per CONTEXT.md (`set_frozen` to `freeze_program`/`freeze_program_release`, `set_freeze_authority` to `freeze_authority_transfer`, `revoke` to `freeze_authority_renounce`, `freeze_account(target, bool)` to `freeze_account`/`freeze_account_release`). Test names are exact. Regenerate with `RISC0_DEV_MODE=1 cargo test --workspace`.

## Hard requirements, Functionality

| # | Requirement (RFP-002) | Satisfied by | Kind |
| --- | --- | --- | --- |
| F1 | Freeze authority set at program initialisation | `bootstrap_writes_valid_config_to_empty_account`, `transfer_installs_signer_candidate` (embedded born-vacant appointment) | test |
| F2 | Freeze authority changed by admin | `transfer_shared_account_emits_single_post_state` (admin-signed), `freeze_authority_transfer_rejects_non_admin_caller` | test |
| F3 | Freeze the program, rejecting interactions except unfreeze and authority changes | `freeze_config_perform_freeze_holder_flips_flag`, `gate_reads_frozen_flag_at_offset`, F3 carve-out list in CONTEXT.md, `auto_sample_idl_pins_gate_shape`; framework side `inject_matches_qualified_wrapper_by_last_segment`, `wrap_stamped_attr_carries_embedded_offset` | test + design |
| F4 | Unfreeze, re-enabling interactions | `freeze_config_perform_release_holder_clears_flag`, `gate_passes_when_unfrozen_at_offset` | test |
| F5 | Freeze authority revoked by admin | `perform_renounce_zeros_slot`, `renounce_shared_account_emits_single_post_state`, dual path per ADR-0004 | test |
| F6 | Freeze a specific account by AccountId, rest operational | `frozen_account_state_perform_freeze_holder_flips_flag`, `gate_per_account_arm_is_offset_free` (program unfrozen, account frozen, rejection fires) | test |
| F7 | Unfreeze a specific account | `frozen_account_state_perform_release_holder_clears_flag`, `from_data_or_default_empty_yields_default_unfrozen` | test |

## Hard requirements, Usability

| # | Requirement | Satisfied by | Kind |
| --- | --- | --- | --- |
| U1 | SPEL integration, minimal boilerplate | `#[freeze_authority]` auto mode, manual mode, `#[freeze_exempt]`, three samples, IDL pins; framework side `role_matched_params_skip_injection`, `role_matched_compound_pda_skips_injection`, `inject_emits_compound_pda_attr` | sample + test |
| U2 | One freeze authority at a time | single `AuthoritySlot` in `FreezeConfig`, transfer semantics tests | design + test |
| U3 | End-to-end usage example in docs | README both modes, `scripts/dry-run.sh` and `scripts/dry-run-embedded.sh` with committed outputs | doc |

## Hard requirements, Performance

| # | Requirement | Satisfied by | Kind |
| --- | --- | --- | --- |
| P1 | Document transaction size overhead of the freeze check | README overhead section, per mode, measured from the committed captures | doc |

## Hard requirements, Supportability

| # | Requirement | Satisfied by | Kind |
| --- | --- | --- | --- |
| S1 | CI green on default branch | `.github/workflows/ci.yml` | ci |
| S2 | Every hard requirement has a test | this matrix | doc |
| S3 | README documents dependency and integration | README, Dependencies table with exact revs | doc |
| S4 | Sample program included | `freeze-authority-sample`, `freeze-authority-sample-manual`, `freeze-authority-sample-embedded` | sample |

## Soft requirement, Reliability

| # | Requirement | Satisfied by | Kind |
| --- | --- | --- | --- |
| R1 | Freeze authority set only to a valid new signer | `transfer_rejects_unauthorized_signer_candidate`, `transfer_rejects_undeployed_pda_candidate`, `transfer_rejects_mismatched_pda_candidate`, `bootstrap_rejects_default_account_id`, `initialize_rejects_default_account_id` | test |

## Proposal scenarios (logos-co/rfp#47)

| Scenario (proposal wording) | Satisfied by |
| --- | --- |
| Initialization with freeze authority | `bootstrap_writes_valid_config_to_empty_account` |
| Initialization without freeze authority (admin signature required) | `freeze_initialize_rejects_non_admin_caller` |
| `set_frozen(true)` success | `freeze_config_perform_freeze_holder_flips_flag` |
| `set_frozen(true)` non-authority rejection | `freeze_config_set_is_frozen_rejects_non_holder` |
| `set_value` rejection while program frozen | `gate_reads_frozen_flag_at_offset`, `gate_without_offset_reads_dedicated_layout` |
| `set_frozen(false)` and restore | `freeze_config_perform_release_holder_clears_flag` |
| `set_value` success after unfreeze | `gate_passes_when_unfrozen_at_offset` |
| `set_freeze_authority` success (admin) | `transfer_shared_account_emits_single_post_state`, `transfer_installs_signer_candidate` |
| `set_freeze_authority` rejection (non-admin) | `freeze_authority_transfer_rejects_non_admin_caller` |
| Revoke success | `perform_renounce_zeros_slot`, `renounce_zeros_slot` |
| `set_frozen` rejection after revoke | `freeze_config_set_is_frozen_rejects_renounced_slot`, `assert_rejects_renounced_slot` |
| `freeze_account(target, true)` success and non-authority rejection | `frozen_account_state_perform_freeze_holder_flips_flag`, `frozen_account_state_set_is_frozen_rejects_non_holder` |
| `set_value` rejection for a frozen account while program unfrozen | `gate_per_account_arm_is_offset_free` |
| `freeze_account(target, false)` and subsequent `set_value` success | `released_account_passes_the_gate_again` (full round-trip through the real gate) |

## M2.5 embedded surface

Delivered with the m2_5 branch set, evidence: the M2.5 PR set, `docs/dry-run-embedded-output.txt`, and:

| Behavior | Satisfied by |
| --- | --- |
| Freeze slot embedded at marker offset, adjacent to admin, neighbors preserved | `value_survives_admin_transfer_and_freeze_appointment`, `freeze_toggle_preserves_admin_window` |
| Born vacant until admin appoints | `freeze_slot_is_born_vacant_until_admin_appoints` |
| Shared account emits one post-state (LEZ duplicate-account rule) | `renounce_shared_account_emits_single_post_state`, `transfer_shared_account_emits_single_post_state`, `shared_account_renounce_emits_single_post_state`, `renounce_distinct_accounts_keeps_both_post_states` |
| Admin's location resolved cross-marker at the consumer's build | framework `cross_marker_bound_resolves_peer_offset`, `cross_marker_bound_without_default_requires_the_peer_marker`, `embedded_role_substitutes_on_peer_extension_fns` |
| Same account at the same offset is a compile error | framework `same_account_same_offset_embeds_are_rejected` |
| Offsets and initializers never in the IDL | `idl_shows_shared_account_surface_without_initializers_or_offsets`; framework side `consumer_offset_kwarg_on_embedded_gate_is_error`, `consumer_location_kwarg_on_embedded_gate_is_error` |
| Dedicated mode unchanged | dedicated dry-run byte-identical to the M2 pin, `manual_sample_idl_matches_auto_gate_shape` |

Every row above names at least one passing test. No known gaps.
