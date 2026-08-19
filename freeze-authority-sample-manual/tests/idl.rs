use std::path::PathBuf;

use spel_framework_core::idl_gen::generate_idl_from_file_with_deps;

/// Pins the IDL shape for the manual-mode sample. Verifies the framework
/// treats explicitly-written `#[require_not_frozen]` attrs the same way
/// as its own auto-wrap: same accounts injected, same names, same order.
/// If auto and manual samples ever diverge in what the IDL sees, the
/// auto-wrap contract is broken.
#[test]
fn manual_sample_idl_matches_auto_gate_shape() {
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

    // Manually-gated `update_value` (carries `#[require_not_frozen]` in
    // source): the wrapper macro plus role-based remap must produce the
    // same four accounts the auto sample sees.
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

    // read_value in manual mode has no explicit gate attr, so it must
    // stay ungated. Confirms the manual mode really is opt-in per fn.
    let read = idl
        .instructions
        .iter()
        .find(|i| i.name == "read_value")
        .unwrap();
    let read_accounts: Vec<&str> = read.accounts.iter().map(|a| a.name.as_str()).collect();
    for forbidden in ["freeze_config", "freeze_account"] {
        assert!(
            !read_accounts.contains(&forbidden),
            "read_value in manual mode must stay ungated but declares `{forbidden}`: {read_accounts:?}"
        );
    }
}
