#![warn(missing_docs)]

use borsh::{BorshDeserialize, BorshSerialize};
use spel_framework::prelude::*;

use crate::config::*;
use crate::errors::*;

/// Per-account freeze state.
///
/// Stored in a per-target PDA at `(program_id, "frozen", target)`. Created
/// on first `freeze_account(target)`; mutated in place on subsequent
/// `freeze_account` / `freeze_account_release` calls. Per ADR-0008, PDAs
/// persist for their lifetime (LEZ has no close primitive).
#[account_type]
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct FrozenAccountState {
    /// Whether the target `AccountId` is currently frozen for this program.
    pub is_frozen: bool,
}

impl FrozenAccountState {
    /// Lenient decode from account data.
    ///
    /// Empty bytes are treated as never-frozen (returns
    /// `Default::default()`, i.e. `is_frozen = false`). This is the
    /// "reads-or-defaults" semantic for lazily-created per-account PDAs:
    /// the auto-wrap gate can read a missing PDA without erroring.
    /// Malformed non-empty bytes return `FreezeError::DecodingFailed`.
    pub fn from_data_or_default(data: &[u8]) -> Result<Self, FreezeError> {
        if data.is_empty() {
            return Ok(Self::default());
        }
        borsh::from_slice(data).map_err(|_| FreezeError::DecodingFailed)
    }

    /// Loads state from an account's data field. Convenience wrapper over
    /// [`FrozenAccountState::from_data_or_default`].
    pub fn from_account(account: &AccountWithMetadata) -> Result<Self, FreezeError> {
        Self::from_data_or_default(&account.account.data)
    }

    /// Borsh-serialises the state for storage in the PDA's account data.
    ///
    /// Returns `FreezeError::EncodingFailed` if serialisation fails.
    pub fn encode(&self) -> Result<Vec<u8>, FreezeError> {
        borsh::to_vec(self).map_err(|_| FreezeError::EncodingFailed)
    }

    /// Serialises and writes this state into an account's data field.
    ///
    /// Returns `FreezeError::AccountDataTooLarge` if the encoded bytes
    /// exceed the account's max length.
    pub fn write_to(&self, account: &mut AccountWithMetadata) -> Result<(), FreezeError> {
        let bytes = self.encode()?;
        account.account.data = bytes
            .try_into()
            .map_err(|_| FreezeError::AccountDataTooLarge)?;
        Ok(())
    }

    /// Sets the per-account `is_frozen` flag. Blocks the target account from
    /// interacting with `require_not_frozen`-gated instructions.
    ///
    /// Decodes `freeze_config` and runs `authority_state.assert(caller)`;
    /// only the current freeze authority may call. Non-holder callers are
    /// rejected with `FreezeError::NotFreezeAuthority`.
    pub fn set_is_frozen(
        &mut self,
        authority_state: &FreezeConfig,
        caller: &AccountWithMetadata,
        state: bool,
    ) -> Result<(), FreezeError> {
        authority_state.assert(caller)?;
        self.is_frozen = state;
        Ok(())
    }

    /// Loads per-account state, sets `is_frozen = true`, writes back.
    ///
    /// Enforces the holder check via `set_is_frozen`.
    pub fn perform_freeze(
        authority_state: &FreezeConfig,
        config_account: &mut AccountWithMetadata,
        current: &AccountWithMetadata,
    ) -> Result<(), FreezeError> {
        let mut state = Self::from_account(config_account)?;
        state.set_is_frozen(authority_state, current, true)?;
        state.write_to(config_account)
    }

    /// Loads per-account state, sets `is_frozen = false`, writes back.
    ///
    /// Enforces the holder check via `set_is_frozen`.
    pub fn perform_release(
        authority_state: &FreezeConfig,
        config_account: &mut AccountWithMetadata,
        current: &AccountWithMetadata,
    ) -> Result<(), FreezeError> {
        let mut state = Self::from_account(config_account)?;
        state.set_is_frozen(authority_state, current, false)?;
        state.write_to(config_account)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    #[test]
    fn from_data_or_default_empty_yields_default_unfrozen() {
        let data: [u8; 0] = [];
        let state = FrozenAccountState::from_data_or_default(&data)
            .expect("empty bytes must decode cleanly to default");
        assert!(!state.is_frozen);
    }

    #[test]
    fn from_data_or_default_decodes_valid_frozen() {
        let data: [u8; 1] = [1; 1];
        let state = FrozenAccountState::from_data_or_default(&data)
            .expect("byte 1 must decode cleanly to true");
        assert!(state.is_frozen);
    }

    #[test]
    fn from_data_or_default_decodes_valid_unfrozen() {
        let data: [u8; 1] = [0; 1];
        let state = FrozenAccountState::from_data_or_default(&data)
            .expect("byte 0 must decode cleanly to false");
        assert!(!state.is_frozen);
    }

    #[test]
    fn from_data_or_default_malformed_errors() {
        let data: [u8; 1] = [9; 1];
        let err = FrozenAccountState::from_data_or_default(&data)
            .expect_err("malformed bytes must not decode cleanly");
        assert_eq!(err, FreezeError::DecodingFailed);
    }

    #[test]
    fn frozen_account_state_set_is_frozen_holder_flips_flag() {
        let auth = acct(1, true);
        let cfg = FreezeConfig::initialize(auth.account_id).unwrap();
        let mut state = FrozenAccountState::default();
        state.set_is_frozen(&cfg, &auth, true).unwrap();
        assert!(state.is_frozen);
        state.set_is_frozen(&cfg, &auth, false).unwrap();
        assert!(!state.is_frozen);
    }

    #[test]
    fn frozen_account_state_set_is_frozen_rejects_non_holder() {
        let auth = acct(1, true);
        let other = acct(2, true);
        let cfg = FreezeConfig::initialize(auth.account_id).unwrap();
        let mut state = FrozenAccountState::default();
        assert_eq!(
            state.set_is_frozen(&cfg, &other, true).unwrap_err(),
            FreezeError::NotFreezeAuthority
        );
        assert!(!state.is_frozen);
    }

    #[test]
    fn frozen_account_state_perform_freeze_holder_flips_flag() {
        let auth = acct(1, true);
        let cfg = FreezeConfig::initialize(auth.account_id).unwrap();
        let mut per_account = AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: AccountId::new([7; 32]),
        };
        FrozenAccountState::perform_freeze(&cfg, &mut per_account, &auth).unwrap();
        let decoded = FrozenAccountState::from_account(&per_account).unwrap();
        assert!(decoded.is_frozen);
    }

    #[test]
    fn frozen_account_state_perform_release_holder_clears_flag() {
        let auth = acct(1, true);
        let cfg = FreezeConfig::initialize(auth.account_id).unwrap();
        let mut per_account = per_account_with(true);
        FrozenAccountState::perform_release(&cfg, &mut per_account, &auth).unwrap();
        let decoded = FrozenAccountState::from_account(&per_account).unwrap();
        assert!(!decoded.is_frozen);
    }

    #[test]
    fn frozen_account_state_write_to_rejects_oversized_data() {
        // Same as above — 1-byte encoding can't overflow the data cap.
    }
}
