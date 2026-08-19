//! Calls the compiled, macro-expanded initialize exactly as dispatched,
//! on a fresh account. Regression for the freeze#3 review find: the
//! auto-wrap gate decoded the very account initialize was about to
//! create, so the embedded sample could never be initialized on-chain.
//! The framework now skips a gate on the fn that creates the gate's
//! embedding account, and stops injecting the gate's params there, so
//! this fn's arity is the consumer's own two accounts.

include!("../src/main.rs");

#[test]
fn initialize_succeeds_on_a_fresh_account() {
    let config = AccountWithMetadata {
        account: Account::default(),
        is_authorized: false,
        account_id: AccountId::new([1; 32]),
    };
    let signer = AccountWithMetadata {
        account: Account::default(),
        is_authorized: true,
        account_id: AccountId::new([2; 32]),
    };

    let result = freeze_authority_sample_embedded::initialize(config, signer);
    assert!(
        result.is_ok(),
        "the embedding account's creator must not be gated: {result:?}"
    );
}
