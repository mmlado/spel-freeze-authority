use std::path::PathBuf;

use spel_framework_core::idl_gen::generate_idl_from_file_with_deps;

#[test]
fn idl_shows_shared_account_surface_without_initializers_or_offsets() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let src = PathBuf::from(manifest_dir).join("src/main.rs");

    let deps = [
        PathBuf::from(manifest_dir)
            .join("../../spel-admin-authority/admin-authority")
            .canonicalize()
            .expect("admin-authority dir"),
        PathBuf::from(manifest_dir)
            .join("../freeze-authority")
            .canonicalize()
            .expect("freeze-authority dir"),
        PathBuf::from(manifest_dir)
            .join("../../spel-authority")
            .canonicalize()
            .expect("spel-authority dir"),
    ];
    let idl = generate_idl_from_file_with_deps(&src, &deps).expect("IDL generation failed");

    let names: Vec<&str> = idl.instructions.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "initialize",
            "update_value",
            "read_value",
            "withdraw",
            "admin_transfer",
            "admin_renounce",
            "freeze_authority_transfer",
            "freeze_authority_renounce",
            "freeze_program",
            "freeze_program_release",
            "freeze_account",
            "freeze_account_release",
        ],
        "embedded surface drifted"
    );
    assert!(
        !names.contains(&"admin_initialize") && !names.contains(&"freeze_initialize"),
        "embedded mode must not emit either initializer"
    );

    // Both slots are born inside the consumer's account: the dedicated
    // PDAs and the bound offsets must appear nowhere.
    for ix in &idl.instructions {
        assert!(
            ix.accounts
                .iter()
                .all(|a| a.name != "admin_config" && a.name != "freeze_config"),
            "`{}` still references a dedicated config PDA",
            ix.name
        );
        assert!(
            ix.args
                .iter()
                .all(|a| a.name != "offset" && a.name != "admin_offset"),
            "`{}` leaks a bound offset into the ABI",
            ix.name
        );
    }

    // The merge: dual-role fns list the shared account exactly once.
    for name in ["freeze_authority_transfer", "freeze_authority_renounce"] {
        let ix = idl
            .instructions
            .iter()
            .find(|i| i.name == name)
            .expect(name);
        let config_count = ix.accounts.iter().filter(|a| a.name == "config").count();
        assert_eq!(
            config_count, 1,
            "`{name}` must carry the shared account exactly once, got {config_count}"
        );
    }

    // Wrapped consumer fn: freeze gate params injected around the
    // declared embedding account.
    let update_value = idl
        .instructions
        .iter()
        .find(|i| i.name == "update_value")
        .expect("update_value");
    let account_names: Vec<&str> = update_value
        .accounts
        .iter()
        .map(|a| a.name.as_str())
        .collect();
    assert!(
        account_names.contains(&"config") && account_names.contains(&"caller"),
        "update_value must keep the declared account and gain the injected caller: {account_names:?}"
    );

    // Exempt fn: no freeze wrap, so no injected freeze_account.
    let withdraw = idl
        .instructions
        .iter()
        .find(|i| i.name == "withdraw")
        .expect("withdraw");
    assert!(
        withdraw.accounts.iter().all(|a| a.name != "freeze_account"),
        "freeze_exempt fn must not gain the freeze wrap's params"
    );
}
