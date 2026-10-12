//! The library's own outputs run through LEZ's `validate_execution`, then
//! claims and nonce bumps applied the way a sequencer applies them.

use admin_authority::admin_initialize;
use freeze_authority::{
    FreezeCandidate, FreezeError, freeze_authority_renounce, freeze_authority_transfer,
    freeze_initialize,
};
use nssa_core::account::{Account, AccountId, AccountWithMetadata};
use nssa_core::program::{
    AccountPostState, ExecutionValidationError, PdaSeed, ProgramId, validate_execution,
};
use spel_framework::pda::seed_from_str;
use spel_framework::prelude::{SpelError, SpelResult};

const PROGRAM: ProgramId = [7; 8];
const WALLET_PROGRAM: ProgramId = [5; 8];

fn pda(name: &str) -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account::default(),
        is_authorized: false,
        account_id: AccountId::for_public_pda(&PROGRAM, &PdaSeed::new(seed_from_str(name))),
    }
}

fn never_used(id: u8) -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account::default(),
        is_authorized: true,
        account_id: AccountId::new([id; 32]),
    }
}

fn owned(id: u8) -> AccountWithMetadata {
    let mut key = never_used(id);
    key.account.program_owner = WALLET_PROGRAM;
    key
}

fn signing(mut key: AccountWithMetadata) -> AccountWithMetadata {
    key.is_authorized = true;
    key
}

fn not_signing(mut key: AccountWithMetadata) -> AccountWithMetadata {
    key.is_authorized = false;
    key
}

/// One transaction: LEZ's checks, then claims, then every signer's nonce bump.
fn land(
    pre: Vec<AccountWithMetadata>,
    posts: Vec<AccountPostState>,
) -> Result<Vec<AccountWithMetadata>, ExecutionValidationError> {
    validate_execution(&pre, &posts, PROGRAM)?;
    Ok(pre
        .into_iter()
        .zip(posts)
        .map(|(mut before, post)| {
            let claimed = post.required_claim().is_some();
            before.account = post.account().clone();
            if claimed {
                before.account.program_owner = PROGRAM;
            }
            if before.is_authorized {
                before.account.nonce.public_account_nonce_increment();
            }
            before
        })
        .collect())
}

fn posts(result: SpelResult) -> Vec<AccountPostState> {
    result
        .expect("the library accepts")
        .into_parts()
        .post_states
}

/// Admin and freeze authority both held by an owned key `1`.
/// Returns `[admin_config, freeze_config, admin]`.
fn initialised() -> Vec<AccountWithMetadata> {
    let admin = owned(1);
    let after = land(
        vec![pda("admin_config"), admin.clone()],
        posts(admin_initialize(pda("admin_config"), admin)),
    )
    .unwrap();
    let (admin_config, admin) = (not_signing(after[0].clone()), signing(after[1].clone()));

    let after = land(
        vec![admin_config.clone(), admin.clone(), pda("freeze_config")],
        posts(freeze_initialize(
            admin_config.clone(),
            admin,
            pda("freeze_config"),
        )),
    )
    .unwrap();
    vec![admin_config, after[2].clone(), signing(after[1].clone())]
}

#[test]
fn a_never_used_key_is_refused_as_the_freeze_authority() {
    let [admin_config, freeze_config, admin] = initialised().try_into().unwrap();
    let expected = SpelError::from(FreezeError::UnownedSigner);
    match freeze_authority_transfer(
        admin_config,
        freeze_config,
        admin,
        never_used(2),
        FreezeCandidate::Signer,
        0,
        0,
    ) {
        Err(err) => assert_eq!(err.to_string(), expected.to_string()),
        Ok(_) => panic!("the library accepted a key no program owns"),
    }
}

#[test]
fn lez_rejects_an_unowned_freeze_authority_after_its_first_transaction() {
    // What the refusal prevents. On chain SPEL drops this account first and
    // LEZ answers DeclaredAccountMissingFromOutput instead.
    let [admin_config, freeze_config, admin] = initialised().try_into().unwrap();
    let mut holder = owned(2);
    let pre = vec![
        admin_config.clone(),
        freeze_config.clone(),
        admin.clone(),
        holder.clone(),
    ];
    let after = land(
        pre,
        posts(freeze_authority_transfer(
            admin_config.clone(),
            freeze_config,
            admin,
            holder.clone(),
            FreezeCandidate::Signer,
            0,
            0,
        )),
    )
    .unwrap();
    holder.account.program_owner = ProgramId::default();
    holder.account.nonce = after[3].account.nonce;

    let pre = vec![admin_config.clone(), after[1].clone(), holder.clone()];
    let renounce = posts(freeze_authority_renounce(
        admin_config,
        after[1].clone(),
        holder,
        0,
        0,
    ));
    assert!(matches!(
        validate_execution(&pre, &renounce, PROGRAM),
        Err(ExecutionValidationError::NonDefaultAccountWithDefaultOwner { .. })
    ));
}

#[test]
fn an_owned_freeze_authority_keeps_working_after_a_transfer() {
    let [admin_config, freeze_config, admin] = initialised().try_into().unwrap();
    let holder = owned(2);
    let pre = vec![
        admin_config.clone(),
        freeze_config.clone(),
        admin.clone(),
        holder.clone(),
    ];
    let after = land(
        pre,
        posts(freeze_authority_transfer(
            admin_config.clone(),
            freeze_config,
            admin,
            holder,
            FreezeCandidate::Signer,
            0,
            0,
        )),
    )
    .unwrap();

    let holder = signing(after[3].clone());
    let pre = vec![admin_config.clone(), after[1].clone(), holder.clone()];
    let renounce = posts(freeze_authority_renounce(
        admin_config,
        after[1].clone(),
        holder,
        0,
        0,
    ));
    land(pre, renounce).expect("LEZ accepts the new holder's next transaction");
}
