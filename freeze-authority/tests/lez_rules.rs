//! The library's own outputs run through LEZ's `validate_execution`, then
//! claims and nonce bumps applied the way a sequencer applies them.

use admin_authority::admin_initialize;
use freeze_authority::{
    FreezeCandidate, FreezeError, freeze_authority_accept, freeze_authority_nominate,
    freeze_authority_renounce, freeze_authority_transfer, freeze_initialize,
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

const HOLDER_PROGRAM: ProgramId = [9; 8];

/// Freeze authority handed from the admin (key `1`) to owned key `2`.
/// Returns `[admin_config, freeze_config, admin, holder]`.
fn handed_to_holder() -> Vec<AccountWithMetadata> {
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
            admin.clone(),
            holder,
            FreezeCandidate::Signer,
            0,
            0,
        )),
    )
    .unwrap();
    vec![
        admin_config,
        after[1].clone(),
        admin,
        signing(after[3].clone()),
    ]
}

fn nominate(
    admin_config: &AccountWithMetadata,
    freeze_config: &AccountWithMetadata,
    by: &AccountWithMetadata,
    nominee: &AccountWithMetadata,
) -> AccountWithMetadata {
    let pre = vec![admin_config.clone(), freeze_config.clone(), by.clone()];
    let after = land(
        pre,
        posts(freeze_authority_nominate(
            admin_config.clone(),
            freeze_config.clone(),
            by.clone(),
            *nominee.account_id.value(),
            0,
            0,
        )),
    )
    .unwrap();
    after[1].clone()
}

/// The nominee accepts, then renounces as its next transaction.
fn accept_then_act(
    admin_config: &AccountWithMetadata,
    freeze_config: AccountWithMetadata,
    nominee: AccountWithMetadata,
) {
    let after = land(
        vec![freeze_config.clone(), nominee.clone()],
        posts(freeze_authority_accept(freeze_config, nominee, 0)),
    )
    .unwrap();
    let holder = signing(after[1].clone());
    let pre = vec![admin_config.clone(), after[0].clone(), holder.clone()];
    let renounce = posts(freeze_authority_renounce(
        admin_config.clone(),
        after[0].clone(),
        holder,
        0,
        0,
    ));
    land(pre, renounce).expect("LEZ accepts the new holder's next transaction");
}

#[test]
fn the_holder_hands_over_in_two_steps() {
    let [admin_config, freeze_config, _, holder] = handed_to_holder().try_into().unwrap();
    let nominee = owned(3);
    let freeze_config = nominate(&admin_config, &freeze_config, &holder, &nominee);
    accept_then_act(&admin_config, freeze_config, nominee);
}

#[test]
fn the_admin_fills_a_vacant_slot_in_two_steps() {
    let [admin_config, freeze_config, admin, holder] = handed_to_holder().try_into().unwrap();
    let after = land(
        vec![admin_config.clone(), freeze_config.clone(), holder.clone()],
        posts(freeze_authority_renounce(
            admin_config.clone(),
            freeze_config,
            holder,
            0,
            0,
        )),
    )
    .unwrap();
    let nominee = owned(3);
    let freeze_config = nominate(&admin_config, &after[1], &admin, &nominee);
    accept_then_act(&admin_config, freeze_config, nominee);
}

#[test]
fn a_never_used_nominee_is_refused_at_accept() {
    let [admin_config, freeze_config, _, holder] = handed_to_holder().try_into().unwrap();
    let nominee = never_used(3);
    let freeze_config = nominate(&admin_config, &freeze_config, &holder, &nominee);
    let expected = SpelError::from(FreezeError::UnownedSigner);
    match freeze_authority_accept(freeze_config, nominee, 0) {
        Err(err) => assert_eq!(err.to_string(), expected.to_string()),
        Ok(_) => panic!("the library accepted a key no program owns"),
    }
}

#[test]
fn a_stale_nominee_cannot_accept_after_a_transfer() {
    let [admin_config, freeze_config, admin, holder] = handed_to_holder().try_into().unwrap();
    let stale = owned(3);
    let freeze_config = nominate(&admin_config, &freeze_config, &holder, &stale);
    let next = owned(4);
    let pre = vec![
        admin_config.clone(),
        freeze_config.clone(),
        admin.clone(),
        next.clone(),
    ];
    let after = land(
        pre,
        posts(freeze_authority_transfer(
            admin_config,
            freeze_config,
            admin,
            next,
            FreezeCandidate::Signer,
            0,
            0,
        )),
    )
    .unwrap();
    let expected = SpelError::from(FreezeError::NoNomination);
    match freeze_authority_accept(after[1].clone(), stale, 0) {
        Err(err) => assert_eq!(err.to_string(), expected.to_string()),
        Ok(_) => panic!("a stale nominee took the freeze authority after a transfer"),
    }
}

#[test]
fn a_pda_nominee_accepts_through_its_owning_program() {
    // A chained call with the PDA's seeds reaches the library as a signed,
    // program-owned account.
    let [admin_config, freeze_config, _, holder] = handed_to_holder().try_into().unwrap();
    let pda = AccountWithMetadata {
        account: Account {
            program_owner: HOLDER_PROGRAM,
            data: vec![1].try_into().unwrap(),
            ..Account::default()
        },
        is_authorized: true,
        account_id: AccountId::for_public_pda(
            &HOLDER_PROGRAM,
            &PdaSeed::new(seed_from_str("vault")),
        ),
    };
    let freeze_config = nominate(&admin_config, &freeze_config, &holder, &pda);
    accept_then_act(&admin_config, freeze_config, pda);
}
