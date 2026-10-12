//! Crate-internal test helpers: canned accounts and pre-populated
//! configs shared by the unit tests across modules.

use admin_authority::AdminConfig;
use spel_framework::prelude::*;

use crate::{FreezeConfig, FrozenAccountState};

// A signing key is owned by some program, as `wallet auth-transfer init` leaves it.
pub(crate) fn acct(id_byte: u8, signed: bool) -> AccountWithMetadata {
    let program_owner = if signed { [5; 8] } else { [0; 8] };
    AccountWithMetadata {
        account: Account {
            program_owner,
            ..Account::default()
        },
        is_authorized: signed,
        account_id: AccountId::new([id_byte; 32]),
    }
}

pub(crate) fn config_account_with(cfg: &FreezeConfig) -> AccountWithMetadata {
    AccountWithMetadata {
        account: Account {
            data: borsh::to_vec(cfg).unwrap().try_into().unwrap(),
            ..Account::default()
        },
        is_authorized: false,
        account_id: AccountId::new([9; 32]),
    }
}

pub(crate) fn admin_account_with(admin_id_byte: u8) -> AccountWithMetadata {
    let cfg = AdminConfig::initialize(AccountId::new([admin_id_byte; 32])).unwrap();
    AccountWithMetadata {
        account: Account {
            data: cfg.encode().unwrap().try_into().unwrap(),
            ..Account::default()
        },
        is_authorized: false,
        account_id: AccountId::new([255; 32]),
    }
}

pub(crate) fn per_account_with(is_frozen: bool) -> AccountWithMetadata {
    let state = FrozenAccountState { is_frozen };
    AccountWithMetadata {
        account: Account {
            data: borsh::to_vec(&state).unwrap().try_into().unwrap(),
            ..Account::default()
        },
        is_authorized: false,
        account_id: AccountId::new([7; 32]),
    }
}
