use std::path::PathBuf;

use spel_framework_core::idl::IdlSeed;
use spel_framework_core::idl_gen::generate_idl_from_file_with_deps;

/// Pins the IDL shape for the freeze-authority auto-mode sample. Guards
/// against regressions on the three review-blocking behaviours the
/// framework changes introduced:
///
/// - Every management instruction the freeze library exposes is present.
/// - Auto-gated consumer instructions carry the freeze gate accounts
///   (freeze_config + freeze_account + caller) alongside their own params.
/// - `#[freeze_exempt]` instructions do not receive the gate accounts.
/// - `freeze_program` has no duplicate injections when the source already
///   declares matching PDA and signer params.
#[test]
fn auto_sample_idl_pins_gate_shape() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let src = PathBuf::from(manifest_dir).join("src/main.rs");
    let idl = generate_idl_from_file_with_deps(&src, &[]).expect("IDL generation failed");

    let names: Vec<&str> = idl.instructions.iter().map(|i| i.name.as_str()).collect();

    for expected in [
        "initialize",
        "update_value",
        "read_value",
        "admin_initialize",
        "admin_transfer",
        "admin_renounce",
        "freeze_initialize",
        "freeze_authority_transfer",
        "freeze_authority_renounce",
        "freeze_program",
        "freeze_program_release",
        "freeze_account",
        "freeze_account_release",
    ] {
        assert!(
            names.contains(&expected),
            "missing instruction `{expected}`; got {names:?}"
        );
    }

    // Auto-gated consumer fn: framework must inject freeze_config,
    // freeze_account, and caller alongside the consumer's own `config`.
    let update = idl
        .instructions
        .iter()
        .find(|i| i.name == "update_value")
        .unwrap();
    let update_accounts: Vec<&str> = update.accounts.iter().map(|a| a.name.as_str()).collect();
    for expected in ["freeze_config", "freeze_account", "caller", "config"] {
        assert!(
            update_accounts.contains(&expected),
            "update_value missing `{expected}` account; got {update_accounts:?}"
        );
    }

    let freeze_account = update
        .accounts
        .iter()
        .find(|a| a.name == "freeze_account")
        .unwrap();
    let pda = freeze_account
        .pda
        .as_ref()
        .expect("freeze_account must be a PDA account");
    assert!(
        matches!(
            &pda.seeds[..],
            [
                IdlSeed::Const { value: c },
                IdlSeed::Account { path: a },
            ] if c == "frozen" && a == "caller"
        ),
        "freeze_account PDA seed shape changed: {:?}",
        pda.seeds
    );

    // #[freeze_exempt] must suppress injection.
    let read = idl
        .instructions
        .iter()
        .find(|i| i.name == "read_value")
        .unwrap();
    let read_accounts: Vec<&str> = read.accounts.iter().map(|a| a.name.as_str()).collect();
    for forbidden in ["freeze_config", "freeze_account"] {
        assert!(
            !read_accounts.contains(&forbidden),
            "read_value must be freeze-exempt but declares `{forbidden}`: {read_accounts:?}"
        );
    }

    // freeze_program (from the extension crate) declares its own accounts
    // matching the freeze gate roles. Role-based remap must not duplicate.
    let freeze_program = idl
        .instructions
        .iter()
        .find(|i| i.name == "freeze_program")
        .unwrap();
    let fp_accounts: Vec<&str> = freeze_program
        .accounts
        .iter()
        .map(|a| a.name.as_str())
        .collect();
    assert_eq!(
        freeze_program.accounts.len(),
        3,
        "freeze_program must have exactly 3 accounts, got {fp_accounts:?}"
    );

    // freeze_authority_renounce keeps the dual-path signature: admin_config
    // for the admin path plus freeze_config + caller. Reviewer explicitly
    // asked for this instruction to remain present after the M2 restore.
    let renounce = idl
        .instructions
        .iter()
        .find(|i| i.name == "freeze_authority_renounce")
        .unwrap();
    let renounce_accounts: Vec<&str> = renounce.accounts.iter().map(|a| a.name.as_str()).collect();
    for expected in ["admin_config", "freeze_config", "caller"] {
        assert!(
            renounce_accounts.contains(&expected),
            "freeze_authority_renounce missing `{expected}`; got {renounce_accounts:?}"
        );
    }
}
