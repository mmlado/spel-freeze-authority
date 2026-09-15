//! Freeze authority library for LEZ programs.
//!
//! Ships:
//! - `FreezeConfig` (program-wide freeze authority + frozen flag) and
//!   `FrozenAccountState` (per-account frozen flag).
//! - The `require_not_frozen` gate macro and the `#[freeze_authority]` /
//!   `#[freeze_exempt]` extension attributes.
//! - Seven management instructions covering the RFP-002 admin-supported
//!   flavor: `freeze_initialize`, `freeze_authority_transfer`,
//!   `freeze_authority_renounce`, `freeze_program`, `freeze_program_release`,
//!   `freeze_account`, `freeze_account_release`.

#![warn(missing_docs)]

use admin_authority::{AdminConfig, require_admin};
use spel_framework::prelude::*;

pub use freeze_authority_macros::{
    freeze_authority, freeze_exempt, freeze_initialize, instruction, require_not_frozen,
};

extern crate self as freeze_authority;

mod config;
mod errors;
mod frozen_account;

#[cfg(test)]
mod test_utils;

pub use config::*;
pub use errors::*;
pub use frozen_account::*;

/// Creates the freeze Config PDA and sets the first freeze authority.
///
/// Must be called once per program deployment, after `admin_initialize`. Per
/// ADR-0006 the call requires the current admin's signature; this closes the
/// front-running window that would otherwise let a third party become the
/// freeze authority between `admin_initialize` and `freeze_initialize`.
///
/// `new_freeze_authority` declares the intended authority; `new_freeze_account`
/// is the chain-state evidence the candidate is real. Re-initialisation is
/// rejected automatically by `#[account(init)]`.
#[require_admin]
#[instruction]
#[freeze_exempt]
pub fn freeze_initialize(
    #[account(pda = literal("admin_config"))] admin_config: AccountWithMetadata,
    #[account(signer)] caller: AccountWithMetadata,
    #[account(init, pda = literal("freeze_config"))] mut freeze_config: AccountWithMetadata,
) -> SpelResult {
    FreezeConfig::bootstrap(&mut freeze_config, FreezeCandidate::Signer, &caller)?;
    Ok(SpelOutput::execute(
        vec![
            (admin_config.account, AutoClaim::None),
            (caller.account, AutoClaim::None),
            (
                freeze_config.account,
                AutoClaim::Claimed(Claim::Pda(PdaSeed::new(seed_from_str("freeze_config")))),
            ),
        ],
        vec![],
    ))
}

/// Installs a validated candidate as the new freeze authority.
///
/// Admin-gated: the caller must sign as the current admin, decoded from
/// `admin_config` at `admin_offset`. Per ADR-0007 this is also the recovery
/// path for a renounced slot. `new_account` carries the chain-state evidence
/// that `candidate` is real. F3 carve-out: callable while frozen.
#[instruction]
#[freeze_exempt]
pub fn freeze_authority_transfer(
    #[account(pda = literal("admin_config"))] admin_config: AccountWithMetadata,
    #[account(mut, pda = literal("freeze_config"))] mut freeze_config: AccountWithMetadata,
    #[account(signer)] caller: AccountWithMetadata,
    new_account: AccountWithMetadata,
    candidate: FreezeCandidate,
    offset: usize,
    admin_offset: usize,
) -> SpelResult {
    let admin = AdminConfig::from_account_at(&admin_config, admin_offset)?;
    admin.assert_admin(&caller)?;
    FreezeConfig::perform_transfer_at(&mut freeze_config, offset, candidate, &new_account)?;
    let mut accounts = post_state_pair(admin_config, freeze_config);
    accounts.push(caller.account);
    accounts.push(new_account.account);
    Ok(SpelOutput::execute(accounts, vec![]))
}

/// Vacates the freeze authority slot, zeroing the holder.
///
/// Callable by the current freeze authority, or by the admin when one is
/// decodable from `admin_config`. Unlike the admin role, Renounced is not
/// terminal here: the admin can reinstall a holder via
/// `freeze_authority_transfer` (ADR-0007). F3 carve-out: callable while
/// frozen.
#[instruction]
#[freeze_exempt]
pub fn freeze_authority_renounce(
    #[account(pda = literal("admin_config"))] admin_config: AccountWithMetadata,
    #[account(mut, pda = literal("freeze_config"))] mut freeze_config: AccountWithMetadata,
    #[account(signer)] caller: AccountWithMetadata,
    offset: usize,
    admin_offset: usize,
) -> SpelResult {
    let admin = AdminConfig::from_account_at(&admin_config, admin_offset).ok();
    FreezeConfig::perform_renounce_at(admin.as_ref(), &mut freeze_config, offset, &caller)?;
    let mut accounts = post_state_pair(admin_config, freeze_config);
    accounts.push(caller.account);
    Ok(SpelOutput::execute(accounts, vec![]))
}

/// Sets the program-wide frozen flag to `true`.
///
/// Only the current freeze authority can call. While `is_frozen` is `true`,
/// every dispatched instruction except the F3 carve-outs and admin operations
/// rejects via the auto-wrap framework hook.
///
/// `freeze_account` is this fn's own gate account (library fns declare their
/// gate accounts, ADR-0010) and passes through the post-states unchanged, in
/// declared position. LEZ matches pre and post states by position, so
/// dropping it shifts every later post-state one slot left.
#[instruction]
pub fn freeze_program(
    #[account(mut, pda = literal("freeze_config"))] mut freeze_config: AccountWithMetadata,
    #[account(pda = [literal("frozen"), account("caller")])] freeze_account: AccountWithMetadata,
    #[account(signer)] caller: AccountWithMetadata,
    offset: usize,
) -> SpelResult {
    FreezeConfig::perform_freeze_at(&mut freeze_config, offset, &caller)?;
    Ok(SpelOutput::execute(
        vec![
            freeze_config.account,
            freeze_account.account,
            caller.account,
        ],
        vec![],
    ))
}

/// Sets the program-wide frozen flag to `false`.
///
/// Only the current freeze authority can call. F3 carve-out: callable while
/// the program is frozen.
#[instruction]
#[freeze_exempt]
pub fn freeze_program_release(
    #[account(mut, pda = literal("freeze_config"))] mut freeze_config: AccountWithMetadata,
    #[account(signer)] caller: AccountWithMetadata,
    offset: usize,
) -> SpelResult {
    FreezeConfig::perform_release_at(&mut freeze_config, offset, &caller)?;
    Ok(SpelOutput::execute(
        vec![freeze_config.account, caller.account],
        vec![],
    ))
}

/// Sets the per-account frozen flag to `true` for `target`.
///
/// Only the current freeze authority can call. First call against a target
/// inits the per-account PDA; subsequent calls toggle the bool in place
/// (PDAs persist per ADR-0008; LEZ has no close primitive). The first touch
/// emits the marker's post-state with a `Claim::Pda` built from the same
/// seeds the account declares — LEZ rejects a write to a default account
/// without a claim. Toggles emit it unclaimed. F3 carve-out:
/// callable while the program-wide frozen flag is `true` — useful for
/// preparing per-account blocks before unfreezing the program.
#[instruction]
#[freeze_exempt]
pub fn freeze_account(
    #[account(pda = literal("freeze_config"))] freeze_config: AccountWithMetadata,
    #[account(mut, pda = [literal("frozen"), arg("target")])] mut frozen_pda: AccountWithMetadata,
    #[account(signer)] caller: AccountWithMetadata,
    target: [u8; 32],
    offset: usize,
) -> SpelResult {
    let first_touch = frozen_pda.account.data.is_empty();
    let authority = FreezeConfig::from_account_at(&freeze_config, offset)?;
    FrozenAccountState::perform_freeze(&authority, &mut frozen_pda, &caller)?;
    let claim = if first_touch {
        AutoClaim::pda_from_seeds(&[&seed_from_str("frozen"), &target.to_seed()])
    } else {
        AutoClaim::None
    };
    Ok(SpelOutput::execute(
        vec![
            (freeze_config.account, AutoClaim::None),
            (frozen_pda.account, claim),
            (caller.account, AutoClaim::None),
        ],
        vec![],
    ))
}

/// Sets the per-account frozen flag to `false` for `target`.
///
/// Only the current freeze authority can call. Mutates the existing
/// per-target PDA; rejects with `AccountNotFrozen` if the target is not
/// currently frozen — never frozen or already released. F3 carve-out:
/// callable while the program-wide frozen flag is `true`.
#[instruction]
#[freeze_exempt]
pub fn freeze_account_release(
    #[account(pda = literal("freeze_config"))] freeze_config: AccountWithMetadata,
    #[account(mut, pda = [literal("frozen"), arg("target")])] mut frozen_pda: AccountWithMetadata,
    #[account(signer)] caller: AccountWithMetadata,
    target: [u8; 32],
    offset: usize,
) -> SpelResult {
    let _ = target;
    let authority = FreezeConfig::from_account_at(&freeze_config, offset)?;
    FrozenAccountState::perform_release(&authority, &mut frozen_pda, &caller)?;
    Ok(SpelOutput::execute(
        vec![freeze_config.account, frozen_pda.account, caller.account],
        vec![],
    ))
}

/// Post-state pair for a dual-role fn: collapses to the written copy
/// when both roles arrived as copies of one shared embedding account
/// (LEZ rejects duplicate account ids in the output).
fn post_state_pair(read: AccountWithMetadata, written: AccountWithMetadata) -> Vec<Account> {
    if read.account_id == written.account_id {
        vec![written.account]
    } else {
        vec![read.account, written.account]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    // Step-3 gate tests. Red until `require_not_frozen` parses the
    // `offset` int kwarg; they pin the target behavior: one prologue
    // shape, `from_account_at(&cfg, offset)`, offset defaulting to 0.

    fn embedded_config_account(offset: usize, cfg: &FreezeConfig) -> AccountWithMetadata {
        let mut data = vec![0xAA; offset]; // consumer prefix bytes
        data.extend(borsh::to_vec(cfg).unwrap()); // slot + flag window
        data.extend([0xBB; 7]); // consumer suffix bytes
        AccountWithMetadata {
            account: Account {
                data: data.try_into().unwrap(),
                ..Account::default()
            },
            is_authorized: false,
            account_id: AccountId::new([9; 32]),
        }
    }

    #[test]
    fn gate_reads_frozen_flag_at_offset() {
        #[require_not_frozen(freeze_config = cfg, freeze_account = pda, offset = 32)]
        fn gated(cfg: AccountWithMetadata, pda: AccountWithMetadata) -> Result<(), SpelError> {
            let _ = (&cfg, &pda);
            Ok(())
        }
        let mut frozen = FreezeConfig::initialize(AccountId::new([1; 32])).unwrap();
        frozen.is_frozen = true;
        let cfg = embedded_config_account(32, &frozen);
        let err = gated(cfg, acct(7, false)).unwrap_err();
        assert!(
            matches!(err, SpelError::Unauthorized { ref message } if message == "program is frozen")
        );
    }

    #[test]
    fn gate_passes_when_unfrozen_at_offset() {
        #[require_not_frozen(freeze_config = cfg, freeze_account = pda, offset = 32)]
        fn gated(cfg: AccountWithMetadata, pda: AccountWithMetadata) -> Result<(), SpelError> {
            let _ = (&cfg, &pda);
            Ok(())
        }
        let unfrozen = FreezeConfig::initialize(AccountId::new([1; 32])).unwrap();
        let cfg = embedded_config_account(32, &unfrozen);
        assert!(gated(cfg, acct(7, false)).is_ok());
    }

    #[test]
    fn gate_without_offset_reads_dedicated_layout() {
        #[require_not_frozen(freeze_config = cfg, freeze_account = pda)]
        fn gated(cfg: AccountWithMetadata, pda: AccountWithMetadata) -> Result<(), SpelError> {
            let _ = (&cfg, &pda);
            Ok(())
        }
        let mut frozen = FreezeConfig::initialize(AccountId::new([1; 32])).unwrap();
        frozen.is_frozen = true;
        let cfg = config_account_with(&frozen);
        let err = gated(cfg, acct(7, false)).unwrap_err();
        assert!(
            matches!(err, SpelError::Unauthorized { ref message } if message == "program is frozen")
        );
    }

    #[test]
    fn gate_per_account_arm_is_offset_free() {
        // Program-wide unfrozen at offset 32, but the caller's per-account
        // PDA is frozen. The per-account arm must still fire in embedded
        // mode: FrozenAccountState never embeds, its read takes no offset.
        #[require_not_frozen(freeze_config = cfg, freeze_account = pda, offset = 32)]
        fn gated(cfg: AccountWithMetadata, pda: AccountWithMetadata) -> Result<(), SpelError> {
            let _ = (&cfg, &pda);
            Ok(())
        }
        let unfrozen = FreezeConfig::initialize(AccountId::new([1; 32])).unwrap();
        let cfg = embedded_config_account(32, &unfrozen);
        let err = gated(cfg, per_account_with(true)).unwrap_err();
        assert!(
            matches!(err, SpelError::Unauthorized { ref message } if message == "account is frozen")
        );
    }

    #[test]
    fn renounce_shared_account_emits_single_post_state() {
        // The same-account cell, called exactly as the dispatcher will
        // call it after the framework merge: one consumer account cloned
        // into both role positions, offsets baked as literals. LEZ
        // accepts exactly one post-state per transaction account, so the
        // output must collapse the two copies to one entry.
        let caller = acct(1, true); // freeze holder signs
        // Consumer layout: 32 prefix bytes, admin slot at 32, freeze at 64.
        let mut data = vec![0xAA; 32];
        let admin_cfg = AdminConfig::initialize(AccountId::new([2; 32])).unwrap();
        data.extend(admin_cfg.encode().unwrap());
        let freeze_cfg = FreezeConfig::initialize(caller.account_id).unwrap();
        data.extend(borsh::to_vec(&freeze_cfg).unwrap());
        let shared = AccountWithMetadata {
            account: Account {
                data: data.try_into().unwrap(),
                ..Account::default()
            },
            is_authorized: false,
            account_id: AccountId::new([9; 32]),
        };
        let out = freeze_authority_renounce(shared.clone(), shared, caller, 64, 32).unwrap();
        assert_eq!(out.post_states.len(), 2);
    }

    #[test]
    fn transfer_shared_account_emits_single_post_state() {
        // Same-account cell for the admin-only path: the caller is the
        // admin read from the shared account's admin window.
        let caller = acct(2, true); // admin signs
        let new_holder = acct(3, true);
        let mut data = vec![0xAA; 32];
        let admin_cfg = AdminConfig::initialize(caller.account_id).unwrap();
        data.extend(admin_cfg.encode().unwrap());
        let freeze_cfg = FreezeConfig::initialize(AccountId::new([1; 32])).unwrap();
        data.extend(borsh::to_vec(&freeze_cfg).unwrap());
        let shared = AccountWithMetadata {
            account: Account {
                data: data.try_into().unwrap(),
                ..Account::default()
            },
            is_authorized: false,
            account_id: AccountId::new([9; 32]),
        };
        let out = freeze_authority_transfer(
            shared.clone(),
            shared,
            caller,
            new_holder,
            FreezeCandidate::Signer,
            64,
            32,
        )
        .unwrap();
        assert_eq!(out.post_states.len(), 3);
    }

    #[test]
    fn renounce_distinct_accounts_keeps_both_post_states() {
        let caller = acct(1, true);
        let admin_account = admin_account_with(2);
        let freeze_cfg = FreezeConfig::initialize(caller.account_id).unwrap();
        let freeze_account = config_account_with(&freeze_cfg);
        let out = freeze_authority_renounce(admin_account, freeze_account, caller, 0, 0).unwrap();
        assert_eq!(out.post_states.len(), 3);
    }

    // Proposal scenario: initialization without freeze authority, the
    // admin signature requirement rejects everyone else (ADR-0006).
    #[test]
    fn freeze_initialize_rejects_non_admin_caller() {
        let admin_config = admin_account_with(1);
        let non_admin = acct(2, true);
        let freeze_config = acct(9, false);
        let err = freeze_initialize(admin_config, non_admin, freeze_config).unwrap_err();
        assert!(
            matches!(err, SpelError::Unauthorized { ref message } if message.contains("not the current admin"))
        );
    }

    // Proposal scenario: set_freeze_authority rejection for a non-admin
    // caller. The strict body check fires before any slot write.
    #[test]
    fn freeze_authority_transfer_rejects_non_admin_caller() {
        let admin_config = admin_account_with(1);
        let non_admin = acct(2, true);
        let freeze_cfg = FreezeConfig::initialize(AccountId::new([3; 32])).unwrap();
        let freeze_account = config_account_with(&freeze_cfg);
        let new_holder = acct(4, true);
        let err = freeze_authority_transfer(
            admin_config,
            freeze_account,
            non_admin,
            new_holder,
            FreezeCandidate::Signer,
            0,
            0,
        )
        .unwrap_err();
        assert!(
            matches!(err, SpelError::Unauthorized { ref message } if message.contains("not the current admin"))
        );
    }

    // Proposal scenario: freeze_account(target, false) and subsequent
    // gated-call success, as one round-trip through the real gate.
    #[test]
    fn released_account_passes_the_gate_again() {
        #[require_not_frozen(freeze_config = cfg, freeze_account = pda)]
        fn gated(cfg: AccountWithMetadata, pda: AccountWithMetadata) -> Result<(), SpelError> {
            let _ = (&cfg, &pda);
            Ok(())
        }
        let holder = acct(1, true);
        let freeze_cfg = FreezeConfig::initialize(holder.account_id).unwrap();
        let cfg_account = config_account_with(&freeze_cfg);
        let mut pda = per_account_with(true);
        assert!(
            gated(cfg_account.clone(), pda.clone()).is_err(),
            "frozen account must be rejected"
        );
        FrozenAccountState::perform_release(&freeze_cfg, &mut pda, &holder).expect("release");
        assert!(
            gated(cfg_account, pda).is_ok(),
            "released account must pass"
        );
    }

    // ADR-0010 alignment self-test. The probe fn hands the wrapper every
    // role kwarg this test knows about: a name the macro rejects is a
    // compile error here, in our own CI, not in a consumer's build. The
    // runtime half asserts the manifest declares exactly the same set,
    // so adding an inject account without teaching the macro (or the
    // reverse) fails this test either way.
    #[test]
    fn wrapper_kwargs_match_declared_inject_accounts() {
        #[require_not_frozen(freeze_config = a, freeze_account = b, caller = c)]
        fn __probe(
            a: AccountWithMetadata,
            b: AccountWithMetadata,
            c: AccountWithMetadata,
        ) -> Result<(), SpelError> {
            let _ = (&a, &b, &c);
            Ok(())
        }

        let specs = spel_framework_core::extension::read_inject_specs(std::path::Path::new(env!(
            "CARGO_MANIFEST_DIR"
        )))
        .expect("inject metadata must parse");
        let mut declared: Vec<&str> = specs
            .iter()
            .flat_map(|s| s.accounts.iter().map(|a| a.role.as_str()))
            .collect();
        declared.sort_unstable();
        assert_eq!(
            declared,
            vec!["caller", "freeze_account", "freeze_config"],
            "manifest inject accounts drifted from the wrapper's kwarg set"
        );
    }

    // Live-round regression (2026-08-09): freeze_program declared the gate's
    // marker account but emitted only two post-states. LEZ zips pre and post
    // states by position, so the caller's post-state landed in the marker's
    // slot and the sequencer refused with ModifiedNonce.
    #[test]
    fn freeze_program_passes_gate_marker_through_post_states() {
        let holder = acct(1, true);
        let cfg = FreezeConfig::initialize(holder.account_id).unwrap();
        let config_account = embedded_config_account(0, &cfg);
        let marker = acct(7, false);
        let out =
            freeze_program(config_account, marker.clone(), holder, 0).expect("freeze succeeds");
        assert_eq!(
            out.post_states.len(),
            3,
            "one post-state per declared account, in declared order"
        );
        assert_eq!(
            out.post_states[1].account(),
            &marker.account,
            "marker passes through unchanged"
        );
        assert!(
            out.post_states[1].required_claim().is_none(),
            "pass-through must not claim"
        );
        let frozen = FreezeConfig::from_account_at(
            &AccountWithMetadata {
                account: out.post_states[0].account().clone(),
                is_authorized: false,
                account_id: AccountId::new([9; 32]),
            },
            0,
        )
        .expect("decode updated config");
        assert!(frozen.is_frozen, "program-wide flag set");
    }

    // Live-round regression (2026-08-09): the first write to a target's
    // marker PDA must carry a Claim::Pda or LEZ refuses the creation with
    // DefaultAccountModifiedWithoutClaim. Toggles stay unclaimed.
    #[test]
    fn freeze_account_first_touch_emits_pda_claim() {
        let holder = acct(1, true);
        let cfg = FreezeConfig::initialize(holder.account_id).unwrap();
        let config_account = embedded_config_account(0, &cfg);
        let marker = acct(7, false);
        let target = [7u8; 32];
        let out = freeze_account(config_account, marker, holder, target, 0)
            .expect("first freeze succeeds");
        let claim = out.post_states[1]
            .required_claim()
            .expect("first touch must claim the marker");
        let AutoClaim::Claimed(expected) =
            AutoClaim::pda_from_seeds(&[&seed_from_str("frozen"), &target.to_seed()])
        else {
            unreachable!("pda_from_seeds always claims");
        };
        assert_eq!(claim, expected, "claim seeds match the declared account");
    }

    #[test]
    fn freeze_account_toggle_emits_no_claim() {
        let holder = acct(1, true);
        let cfg = FreezeConfig::initialize(holder.account_id).unwrap();
        let config_account = embedded_config_account(0, &cfg);
        let marker = per_account_with(true);
        let out =
            freeze_account(config_account, marker, holder, [7u8; 32], 0).expect("toggle succeeds");
        assert!(
            out.post_states[1].required_claim().is_none(),
            "an existing marker must not be re-claimed"
        );
    }
}
