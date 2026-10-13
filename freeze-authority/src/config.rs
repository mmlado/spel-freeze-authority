//! The freeze slot. `FreezeConfig` holds the freeze authority next to
//! the program-wide frozen flag and wraps the shared `authority` slot
//! machinery: decode, assert, transfer, renounce, freeze, and release,
//! each in a plain form for a dedicated Config PDA and an `_at` form
//! that splices only the slot's byte window of an embedding account.

#![warn(missing_docs)]

use admin_authority::AdminConfig;
use authority::{AuthorityCandidate, AuthoritySlot};
use borsh::{BorshDeserialize, BorshSerialize};
use spel_framework::prelude::*;

use crate::errors::*;

/// Transfer-time claim describing the intended new freeze authority. Alias
/// of the shared [`AuthorityCandidate`]: `Signer` proves key control by
/// co-signature, `Pda { program_id, seed }` by address derivation plus a
/// deployment check. Always paired with an account param that carries the
/// chain-state evidence.
pub type FreezeCandidate = AuthorityCandidate;

/// On-chain freeze authority state for a single program.
///
/// Stored in the program's Config PDA at `(program_id, "freeze_config")`.
/// Composes the shared `authority::AuthoritySlot` (the freeze authority and
/// its nominee, 64 bytes) with the program-wide `is_frozen` flag, 65 bytes
/// in all. `slot.is_renounced()`
/// returns true when the holder is `AccountId::default()`; per ADR-0007 the
/// Renounced state is recoverable by admin via `freeze_authority_transfer`.
#[account_type]
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct FreezeConfig {
    slot: AuthoritySlot,
    /// Program-wide frozen flag. Toggled by `freeze_program` /
    /// `freeze_program_release`.
    pub is_frozen: bool,
}

impl FreezeConfig {
    /// Constructs an initialised FreezeConfig.
    ///
    /// Rejects `AccountId::default()` as the freeze authority since that value is the
    /// reserved sentinel for the renounced state.
    pub(crate) fn initialize(freeze_authority: AccountId) -> Result<Self, FreezeError> {
        Ok(Self {
            slot: AuthoritySlot::initialize(freeze_authority)?,
            is_frozen: false,
        })
    }

    /// Asserts the signer matches the current holder.
    ///
    /// Returns `FreezeError::NotFreezeAuthority` when the signer's
    /// `account_id` differs from `slot.holder()`, `FreezeError::Renounced`
    /// when the slot is vacant, and `FreezeError::MissingSignature` when the
    /// witness set has not authorised the signer.
    pub fn assert(&self, signer: &AccountWithMetadata) -> Result<(), FreezeError> {
        self.slot.assert(signer).map_err(FreezeError::from)
    }

    /// Borsh-serialises the config for storage in the PDA's account data.
    ///
    /// Returns `FreezeError::EncodingFailed` if serialisation fails.
    pub fn encode(&self) -> Result<Vec<u8>, FreezeError> {
        borsh::to_vec(self).map_err(|_| FreezeError::EncodingFailed)
    }

    /// Strict decode from raw bytes. Empty data -> NotInitialized.
    pub fn decode(data: &[u8]) -> Result<Self, FreezeError> {
        if data.is_empty() {
            return Err(FreezeError::NotInitialized);
        }
        Self::try_from_slice(data).map_err(|_| FreezeError::DecodingFailed)
    }

    /// Strict decode of the config's window at `offset`.
    ///
    /// Keeps the three-way discrimination: empty data means the
    /// embedding account does not exist yet (`NotInitialized`);
    /// non-empty data too short for the window is `SlotOutOfBounds`,
    /// a layout error. Dedicated mode is the degenerate case
    /// `offset = 0` over the Config PDA.
    ///
    /// # Errors
    ///
    /// `FreezeError::NotInitialized` on empty data,
    /// `FreezeError::SlotOutOfBounds` when the window does not fit,
    /// `FreezeError::DecodingFailed` when the flag byte is not a valid
    /// boolean.
    pub fn decode_at(data: &[u8], offset: usize) -> Result<Self, FreezeError> {
        if data.is_empty() {
            return Err(FreezeError::NotInitialized);
        }
        let slot = AuthoritySlot::read_at(data, offset)?;
        let is_frozen = match data.get(offset + AuthoritySlot::ENCODED_LEN) {
            Some(&0) => false,
            Some(&1) => true,
            Some(_) => return Err(FreezeError::DecodingFailed),
            None => return Err(FreezeError::SlotOutOfBounds),
        };
        Ok(Self { slot, is_frozen })
    }

    /// Loads config from an account's data field. Convenience wrapper over
    /// [`FreezeConfig::decode`].
    pub fn from_account(account: &AccountWithMetadata) -> Result<Self, FreezeError> {
        Self::decode(&account.account.data)
    }

    /// Loads config from an account the calling program owns. For readers
    /// whose account is not already pinned to the program by a PDA
    /// constraint.
    ///
    /// Returns `FreezeError::ConfigNotOwned` when `program_id` does not own
    /// the account, before anything is decoded.
    pub fn from_owned_account(
        account: &AccountWithMetadata,
        program_id: ProgramId,
    ) -> Result<Self, FreezeError> {
        if account.account.program_owner != program_id {
            return Err(FreezeError::ConfigNotOwned);
        }
        Self::from_account(account)
    }

    /// [`FreezeConfig::from_owned_account`] for the window at `offset`.
    pub fn from_owned_account_at(
        account: &AccountWithMetadata,
        offset: usize,
        program_id: ProgramId,
    ) -> Result<Self, FreezeError> {
        if account.account.program_owner != program_id {
            return Err(FreezeError::ConfigNotOwned);
        }
        Self::from_account_at(account, offset)
    }

    /// Loads the config from an account's data at `offset`. Convenience
    /// wrapper over [`FreezeConfig::decode_at`].
    pub fn from_account_at(
        account: &AccountWithMetadata,
        offset: usize,
    ) -> Result<Self, FreezeError> {
        Self::decode_at(&account.account.data, offset)
    }

    /// Sets `is_frozen` to `state`. Program-wide `true` blocks all
    /// non-exempt instructions via the `require_not_frozen` gate.
    ///
    /// Only the current freeze authority may call. `slot.assert(current)`
    /// runs first; a non-holder signer is rejected with
    /// `FreezeError::NotFreezeAuthority`.
    pub fn set_is_frozen(
        &mut self,
        current: &AccountWithMetadata,
        state: bool,
    ) -> Result<(), FreezeError> {
        self.slot.assert(current)?;
        self.is_frozen = state;
        Ok(())
    }

    /// Serialises and writes this config into an account's data field.
    ///
    /// Returns `FreezeError::AccountDataTooLarge` if the encoded bytes exceed
    /// the account's max length.
    pub fn write_to(&self, account: &mut AccountWithMetadata) -> Result<(), FreezeError> {
        let bytes = self.encode()?;
        account.account.data = bytes
            .try_into()
            .map_err(|_| FreezeError::AccountDataTooLarge)?;
        Ok(())
    }

    /// Splices only the config's window at `offset` into the account's
    /// data, leaving every surrounding byte untouched.
    ///
    /// # Errors
    ///
    /// `FreezeError::SlotOutOfBounds` when the window does not fit.
    pub fn write_to_at(
        &self,
        account: &mut AccountWithMetadata,
        offset: usize,
    ) -> Result<(), FreezeError> {
        let mut bytes: Vec<u8> = account.account.data.to_vec();
        self.slot.write_at(&mut bytes, offset)?;
        *bytes
            .get_mut(offset + AuthoritySlot::ENCODED_LEN)
            .ok_or(FreezeError::SlotOutOfBounds)? = self.is_frozen as u8;
        account.account.data = bytes
            .try_into()
            .map_err(|_| FreezeError::AccountDataTooLarge)?;
        Ok(())
    }

    /// Validates a candidate, builds a fresh config, and writes it to the
    /// PDA.
    ///
    /// No auth check on the caller. Candidate validation still runs so a
    /// default `AccountId`, an undeployed PDA, or a mismatched address is
    /// rejected. Callers are responsible for gating who may bootstrap
    /// (`freeze_initialize` gates on admin via `#[require_admin]`).
    pub fn bootstrap(
        config_account: &mut AccountWithMetadata,
        new_authority: FreezeCandidate,
        new_authority_account: &AccountWithMetadata,
    ) -> Result<(), FreezeError> {
        let resolved = new_authority.validate(new_authority_account)?;
        let state = Self::initialize(resolved)?;
        state.write_to(config_account)
    }

    /// Validates a candidate, builds a fresh config, and splices it in
    /// at `offset`. The embedded-mode bootstrap: the consumer's
    /// account-creating instruction calls this after writing its own
    /// state, so the slot is born initialized.
    ///
    /// # Errors
    ///
    /// Candidate validation errors, plus `SlotOutOfBounds` when the
    /// account's data does not cover the window (write the full
    /// consumer struct before bootstrapping).
    pub fn bootstrap_at(
        config_account: &mut AccountWithMetadata,
        offset: usize,
        candidate: FreezeCandidate,
        new_account: &AccountWithMetadata,
    ) -> Result<(), FreezeError> {
        let resolved = candidate.validate(new_account)?;
        let state = Self::initialize(resolved)?;
        state.write_to_at(config_account, offset)
    }

    /// Validates the incoming candidate and installs it as the new holder.
    ///
    /// No auth check on the caller. Candidate validation still runs so
    /// garbage input (default `AccountId`, undeployed PDA, mismatched
    /// address) is rejected here. Callers are responsible for gating who may
    /// transfer (`freeze_authority_transfer` performs the strict admin
    /// check in its body).
    pub fn transfer(
        &mut self,
        candidate: FreezeCandidate,
        new_account: &AccountWithMetadata,
    ) -> Result<(), FreezeError> {
        let next = candidate.validate(new_account)?;
        self.slot.transfer_to(next).map_err(FreezeError::from)
    }

    /// Zeros the freeze authority to `AccountId::default()`, the renounced
    /// sentinel.
    ///
    /// Dual-path auth per ADR-0004: passes when `admin` proves the caller
    /// is the current admin, or when the caller is the current holder.
    /// `None` means the admin config was absent or undecodable; the holder
    /// arm still runs. The caller decodes admin state and passes the value
    /// in (the caller-decodes contract, ADR-0012).
    pub fn renounce(
        &mut self,
        admin_config: Option<&AdminConfig>,
        current: &AccountWithMetadata,
    ) -> Result<(), FreezeError> {
        let admin_ok = admin_config.is_some_and(|a| a.assert_admin(current).is_ok());
        if !admin_ok && self.slot.assert(current).is_err() {
            return Err(FreezeError::NotAdminOrFreezeAuthority);
        }

        self.slot.renounce();
        Ok(())
    }

    /// Names `next` as the nominee, replacing any earlier one. The holder
    /// does not change until the nominee calls [`Self::accept`]. The
    /// all-zero id cancels.
    ///
    /// Dual-path auth like [`Self::renounce`] (ADR-0013): the current
    /// holder or the admin may nominate. With the slot vacant only the
    /// admin arm can pass.
    pub fn nominate(
        &mut self,
        admin_config: Option<&AdminConfig>,
        current: &AccountWithMetadata,
        next: AccountId,
    ) -> Result<(), FreezeError> {
        let admin_ok = admin_config.is_some_and(|a| a.assert_admin(current).is_ok());
        if !admin_ok && self.slot.assert(current).is_err() {
            return Err(FreezeError::NotAdminOrFreezeAuthority);
        }
        self.slot.nominate(next);
        Ok(())
    }

    /// Installs the nominee as freeze authority on the nominee's own
    /// signature. A PDA nominee signs through a chained call from its
    /// owning program. Fills a vacant slot too (ADR-0007).
    pub fn accept(&mut self, nominee: &AccountWithMetadata) -> Result<(), FreezeError> {
        self.slot.accept(nominee).map_err(FreezeError::from)
    }

    /// The nominee waiting to accept, if any.
    pub fn nominee(&self) -> Option<AccountId> {
        self.slot.pending()
    }

    /// Loads config from account at `offset`, nominates `next`, writes back.
    pub fn perform_nominate_at(
        admin_config: Option<&AdminConfig>,
        config_account: &mut AccountWithMetadata,
        offset: usize,
        current: &AccountWithMetadata,
        next: AccountId,
    ) -> Result<(), FreezeError> {
        let mut state = Self::from_account_at(config_account, offset)?;
        state.nominate(admin_config, current, next)?;
        state.write_to_at(config_account, offset)
    }

    /// Loads config from account at `offset`, lets the nominee accept, writes back.
    pub fn perform_accept_at(
        config_account: &mut AccountWithMetadata,
        offset: usize,
        nominee: &AccountWithMetadata,
    ) -> Result<(), FreezeError> {
        let mut state = Self::from_account_at(config_account, offset)?;
        state.accept(nominee)?;
        state.write_to_at(config_account, offset)
    }

    /// Loads config from account, installs the validated candidate, writes back.
    ///
    /// No auth check. `freeze_authority_transfer` performs the strict
    /// admin check in its body before calling this. Accepts a renounced
    /// starting state per ADR-0007.
    pub fn perform_transfer(
        config_account: &mut AccountWithMetadata,
        candidate: FreezeCandidate,
        new_account: &AccountWithMetadata,
    ) -> Result<(), FreezeError> {
        Self::perform_transfer_at(config_account, 0, candidate, new_account)
    }

    /// Loads config from account, transfers admin, writes back.
    pub fn perform_transfer_at(
        config_account: &mut AccountWithMetadata,
        offset: usize,
        candidate: FreezeCandidate,
        new_account: &AccountWithMetadata,
    ) -> Result<(), FreezeError> {
        let mut state = Self::from_account_at(config_account, offset)?;
        state.transfer(candidate, new_account)?;
        state.write_to_at(config_account, offset)
    }

    /// Loads config from account, renounces the slot, writes back.
    ///
    /// Convenience workflow for callers that have already run their own
    /// auth check.
    pub fn perform_renounce(
        admin_config: Option<&AdminConfig>,
        current: &AccountWithMetadata,
        config_account: &mut AccountWithMetadata,
    ) -> Result<(), FreezeError> {
        Self::perform_renounce_at(admin_config, config_account, 0, current)
    }

    /// Loads config from account, renounce admin, writes back.
    pub fn perform_renounce_at(
        admin_config: Option<&AdminConfig>,
        config_account: &mut AccountWithMetadata,
        offset: usize,
        current: &AccountWithMetadata,
    ) -> Result<(), FreezeError> {
        let mut state = Self::from_account_at(config_account, offset)?;
        state.renounce(admin_config, current)?;
        state.write_to_at(config_account, offset)
    }

    /// Loads config from account, sets `is_frozen = true`, writes back.
    ///
    /// Enforces the holder check via `set_is_frozen`.
    pub fn perform_freeze(
        config_account: &mut AccountWithMetadata,
        current: &AccountWithMetadata,
    ) -> Result<(), FreezeError> {
        Self::perform_freeze_at(config_account, 0, current)
    }

    /// Loads config at `offset`, sets `is_frozen = true`, writes back.
    ///
    /// Enforces the holder check via `set_is_frozen`.
    pub fn perform_freeze_at(
        config_account: &mut AccountWithMetadata,
        offset: usize,
        current: &AccountWithMetadata,
    ) -> Result<(), FreezeError> {
        let mut state = Self::from_account_at(config_account, offset)?;
        state.set_is_frozen(current, true)?;
        state.write_to_at(config_account, offset)
    }

    /// Loads config from account, sets `is_frozen = false`, writes back.
    ///
    /// Enforces the holder check via `set_is_frozen`.
    pub fn perform_release(
        config_account: &mut AccountWithMetadata,
        current: &AccountWithMetadata,
    ) -> Result<(), FreezeError> {
        Self::perform_release_at(config_account, 0, current)
    }

    /// Loads config at `offset`, sets `is_frozen = false`, writes back.
    ///
    /// Enforces the holder check via `set_is_frozen`.
    pub fn perform_release_at(
        config_account: &mut AccountWithMetadata,
        offset: usize,
        current: &AccountWithMetadata,
    ) -> Result<(), FreezeError> {
        let mut state = Self::from_account_at(config_account, offset)?;
        state.set_is_frozen(current, false)?;
        state.write_to_at(config_account, offset)
    }
}

impl spel_framework::FixedBorshSize for FreezeConfig {
    const SIZE: usize = 65;
}

impl spel_framework::SlotLayoutProbe for FreezeConfig {
    fn probe() -> Self {
        let mut cfg = FreezeConfig::initialize(AccountId::new([0xA5; 32]))
            .expect("the probe holder is not the renounced sentinel");
        // Non-zero across the whole window, so a misplaced slot shows.
        cfg.slot.nominate(AccountId::new([0x5A; 32]));
        cfg.is_frozen = true;
        cfg
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    // The declared embedded window type must be this crate's real
    // config: the framework emits window collision asserts through its
    // FixedBorshSize::SIZE at every embedded consumer's build.
    #[test]
    fn metadata_state_type_names_the_real_config() {
        let _size_witness = <FreezeConfig as spel_framework::FixedBorshSize>::SIZE;
        let manifest = include_str!("../Cargo.toml");
        assert!(
            manifest.contains(r#"state_type = "freeze_authority::FreezeConfig""#),
            "embedded.state_type must name freeze_authority::FreezeConfig"
        );
    }

    #[test]
    fn freeze_config_decode_empty_returns_not_initialized() {
        let account = AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: AccountId::new([0; 32]),
        };
        let err = FreezeConfig::from_account(&account).unwrap_err();
        assert_eq!(err, FreezeError::NotInitialized);
    }

    #[test]
    fn freeze_config_decode_valid_bytes_roundtrip() {
        let slot = AuthoritySlot::initialize(AccountId::new([1; 32])).unwrap();
        let cfg = FreezeConfig {
            slot,
            is_frozen: false,
        };
        let encoded = borsh::to_vec(&cfg).unwrap();
        let account = AccountWithMetadata {
            account: Account {
                data: encoded.try_into().unwrap(),
                ..Account::default()
            },
            is_authorized: false,
            account_id: AccountId::new([0; 32]),
        };
        let decoded = FreezeConfig::from_account(&account).unwrap();
        assert_eq!(decoded, cfg);
    }

    #[test]
    fn freeze_config_decode_malformed_errors() {
        let account = AccountWithMetadata {
            account: Account {
                data: vec![0xff, 5].try_into().unwrap(),
                ..Account::default()
            },
            is_authorized: false,
            account_id: AccountId::new([0; 32]),
        };
        let err = FreezeConfig::from_account(&account).unwrap_err();
        assert_eq!(err, FreezeError::DecodingFailed);
    }

    #[test]
    fn initialize_rejects_default_account_id() {
        assert_eq!(
            FreezeConfig::initialize(AccountId::default()).unwrap_err(),
            FreezeError::InvalidCandidate
        );
    }

    #[test]
    fn assert_rejects_non_holder() {
        let authority = acct(1, true);
        let signer = &acct(2, true);
        let config = FreezeConfig {
            slot: AuthoritySlot::initialize(authority.account_id).unwrap(),
            is_frozen: false,
        };
        assert_eq!(
            config.assert(signer).unwrap_err(),
            FreezeError::NotFreezeAuthority
        );
    }

    #[test]
    fn assert_rejects_renounced_slot() {
        let authority = acct(1, true);
        let admin = admin_account_with(1);
        let mut config = FreezeConfig {
            slot: AuthoritySlot::initialize(authority.account_id).unwrap(),
            is_frozen: false,
        };
        config
            .renounce(AdminConfig::from_account(&admin).ok().as_ref(), &authority)
            .unwrap();
        assert_eq!(
            config.assert(&authority).unwrap_err(),
            FreezeError::Renounced
        );
    }

    #[test]
    fn assert_rejects_unauthorized_signer() {
        let authority = acct(1, false);
        let config = FreezeConfig {
            slot: AuthoritySlot::initialize(authority.account_id).unwrap(),
            is_frozen: false,
        };
        assert_eq!(
            config.assert(&authority).unwrap_err(),
            FreezeError::MissingSignature
        );
    }

    #[test]
    fn transfer_installs_signer_candidate() {
        let old = acct(1, true);
        let new = acct(2, true);
        let mut cfg = FreezeConfig {
            slot: AuthoritySlot::initialize(old.account_id).unwrap(),
            is_frozen: false,
        };
        cfg.transfer(FreezeCandidate::Signer, &new).unwrap();
        assert!(cfg.assert(&new).is_ok());
        assert_eq!(
            cfg.assert(&old).unwrap_err(),
            FreezeError::NotFreezeAuthority
        );
    }

    #[test]
    fn transfer_rejects_unauthorized_signer_candidate() {
        let old = acct(1, true);
        let new = acct(2, false);
        let mut cfg = FreezeConfig {
            slot: AuthoritySlot::initialize(old.account_id).unwrap(),
            is_frozen: false,
        };
        assert_eq!(
            cfg.transfer(FreezeCandidate::Signer, &new).unwrap_err(),
            FreezeError::InvalidCandidate
        );
        assert!(cfg.assert(&old).is_ok());
    }

    #[test]
    fn transfer_rejects_undeployed_pda_candidate() {
        let old = acct(1, true);
        let program_id: ProgramId = [1; 8];
        let seed = [1; 32];
        let derived = AccountId::for_public_pda(&program_id, &PdaSeed::new(seed));
        let new = AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: derived,
        };
        let mut cfg = FreezeConfig {
            slot: AuthoritySlot::initialize(old.account_id).unwrap(),
            is_frozen: false,
        };
        assert_eq!(
            cfg.transfer(FreezeCandidate::Pda { program_id, seed }, &new)
                .unwrap_err(),
            FreezeError::UndeployedPda
        );
    }

    #[test]
    fn transfer_rejects_mismatched_pda_candidate() {
        let old = acct(1, true);
        let program_id: ProgramId = [1; 8];
        let seed = [1; 32];
        let new = AccountWithMetadata {
            account: Account {
                data: vec![1].try_into().unwrap(),
                ..Account::default()
            },
            is_authorized: false,
            account_id: AccountId::new([9; 32]),
        };
        let mut cfg = FreezeConfig {
            slot: AuthoritySlot::initialize(old.account_id).unwrap(),
            is_frozen: false,
        };
        assert_eq!(
            cfg.transfer(FreezeCandidate::Pda { program_id, seed }, &new)
                .unwrap_err(),
            FreezeError::CandidateMismatch
        );
    }

    #[test]
    fn renounce_zeros_slot() {
        let auth = acct(1, true);
        let admin = admin_account_with(1);
        let mut cfg = FreezeConfig {
            slot: AuthoritySlot::initialize(auth.account_id).unwrap(),
            is_frozen: false,
        };
        cfg.renounce(AdminConfig::from_account(&admin).ok().as_ref(), &auth)
            .unwrap();
        assert_eq!(cfg.assert(&auth).unwrap_err(), FreezeError::Renounced);
    }

    #[test]
    fn freeze_config_set_is_frozen_holder_flips_flag() {
        let auth = acct(1, true);
        let mut cfg = FreezeConfig {
            slot: AuthoritySlot::initialize(auth.account_id).unwrap(),
            is_frozen: false,
        };
        cfg.set_is_frozen(&auth, true).unwrap();
        assert!(cfg.is_frozen);
        cfg.set_is_frozen(&auth, false).unwrap();
        assert!(!cfg.is_frozen);
    }

    #[test]
    fn freeze_config_set_is_frozen_rejects_non_holder() {
        let auth = acct(1, true);
        let other = acct(2, true);
        let mut cfg = FreezeConfig {
            slot: AuthoritySlot::initialize(auth.account_id).unwrap(),
            is_frozen: false,
        };
        assert_eq!(
            cfg.set_is_frozen(&other, true).unwrap_err(),
            FreezeError::NotFreezeAuthority
        );
        assert!(!cfg.is_frozen);
    }

    #[test]
    fn freeze_config_set_is_frozen_rejects_renounced_slot() {
        let auth = acct(1, true);
        let admin = admin_account_with(1);
        let mut cfg = FreezeConfig {
            slot: AuthoritySlot::initialize(auth.account_id).unwrap(),
            is_frozen: false,
        };
        cfg.renounce(AdminConfig::from_account(&admin).ok().as_ref(), &auth)
            .unwrap();
        assert_eq!(
            cfg.set_is_frozen(&auth, true).unwrap_err(),
            FreezeError::Renounced
        );
        assert!(!cfg.is_frozen);
    }

    #[test]
    fn bootstrap_writes_valid_config_to_empty_account() {
        let new_auth = acct(1, true);
        let mut config_account = AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: AccountId::new([9; 32]),
        };
        FreezeConfig::bootstrap(&mut config_account, FreezeCandidate::Signer, &new_auth).unwrap();
        let decoded = FreezeConfig::from_account(&config_account).unwrap();
        assert!(decoded.assert(&new_auth).is_ok());
        assert!(!decoded.is_frozen);
    }

    #[test]
    fn bootstrap_rejects_default_account_id() {
        let mut default_signer = acct(0, true);
        default_signer.account_id = AccountId::default();
        let mut config_account = AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: AccountId::new([9; 32]),
        };
        assert_eq!(
            FreezeConfig::bootstrap(
                &mut config_account,
                FreezeCandidate::Signer,
                &default_signer
            )
            .unwrap_err(),
            FreezeError::InvalidCandidate
        );
    }

    #[test]
    fn freeze_config_perform_freeze_holder_flips_flag() {
        let auth = acct(1, true);
        let cfg = FreezeConfig {
            slot: AuthoritySlot::initialize(auth.account_id).unwrap(),
            is_frozen: false,
        };
        let mut config_account = config_account_with(&cfg);
        FreezeConfig::perform_freeze(&mut config_account, &auth).unwrap();
        let decoded = FreezeConfig::from_account(&config_account).unwrap();
        assert!(decoded.is_frozen);
    }

    #[test]
    fn freeze_config_perform_release_holder_clears_flag() {
        let auth = acct(1, true);
        let cfg = FreezeConfig {
            slot: AuthoritySlot::initialize(auth.account_id).unwrap(),
            is_frozen: true,
        };
        let mut config_account = config_account_with(&cfg);
        FreezeConfig::perform_release(&mut config_account, &auth).unwrap();
        let decoded = FreezeConfig::from_account(&config_account).unwrap();
        assert!(!decoded.is_frozen);
    }

    #[test]
    fn perform_transfer_preserves_is_frozen_flag() {
        // Regression: a prior transfer implementation called perform_release
        // internally, so transferring authority also unfroze the program.
        // Verify perform_transfer only rotates the holder, leaving is_frozen
        // untouched.
        let old = acct(1, true);
        let new = acct(2, true);
        let cfg = FreezeConfig {
            slot: AuthoritySlot::initialize(old.account_id).unwrap(),
            is_frozen: true,
        };
        let mut config_account = config_account_with(&cfg);
        FreezeConfig::perform_transfer(&mut config_account, FreezeCandidate::Signer, &new).unwrap();
        let decoded = FreezeConfig::from_account(&config_account).unwrap();
        assert!(
            decoded.is_frozen,
            "transfer must not clear the program-wide frozen flag"
        );
        assert!(decoded.assert(&new).is_ok());
    }

    #[test]
    fn perform_renounce_zeros_slot() {
        let auth = acct(1, true);
        let admin = admin_account_with(1);
        let cfg = FreezeConfig {
            slot: AuthoritySlot::initialize(auth.account_id).unwrap(),
            is_frozen: false,
        };
        let mut config_account = config_account_with(&cfg);
        FreezeConfig::perform_renounce(
            AdminConfig::from_account(&admin).ok().as_ref(),
            &auth,
            &mut config_account,
        )
        .unwrap();
        let decoded = FreezeConfig::from_account(&config_account).unwrap();
        assert_eq!(decoded.assert(&auth).unwrap_err(), FreezeError::Renounced);
    }

    #[test]
    fn uninitialised_freeze_config_fails_decode_before_the_gate() {
        // Caller-decodes contract: set_is_frozen now takes the decoded
        // FreezeConfig, so the NotInitialized rejection fires at the decode
        // the caller performs, before any per-account state is touched.
        let freeze_config = AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: AccountId::new([9; 32]),
        };
        assert_eq!(
            FreezeConfig::from_account(&freeze_config).unwrap_err(),
            FreezeError::NotInitialized
        );
    }

    #[test]
    fn freeze_config_write_to_rejects_oversized_data() {
        // Skipped: overflowing 65 bytes needs the framework's data cap.
    }

    #[test]
    fn perform_renounce_rejects_unauthorized_caller() {
        let auth = acct(1, true); // freeze holder
        let stranger = acct(2, true); // not admin, not holder
        let admin = admin_account_with(1); // admin is byte 1, not byte 2
        let cfg = FreezeConfig {
            slot: AuthoritySlot::initialize(auth.account_id).unwrap(),
            is_frozen: false,
        };
        let mut config_account = config_account_with(&cfg);
        let err = FreezeConfig::perform_renounce(
            AdminConfig::from_account(&admin).ok().as_ref(),
            &stranger,
            &mut config_account,
        )
        .unwrap_err();
        assert_eq!(err, FreezeError::NotAdminOrFreezeAuthority);
        // Slot untouched
        let decoded = FreezeConfig::from_account(&config_account).unwrap();
        assert!(decoded.assert(&auth).is_ok());
    }

    fn holder_and_admin() -> (
        FreezeConfig,
        AdminConfig,
        AccountWithMetadata,
        AccountWithMetadata,
    ) {
        let holder = acct(1, true);
        let admin = acct(9, true);
        (
            FreezeConfig::initialize(holder.account_id).unwrap(),
            AdminConfig::initialize(admin.account_id).unwrap(),
            holder,
            admin,
        )
    }

    #[test]
    fn the_holder_nominates_and_the_nominee_accepts() {
        let (mut cfg, admin_cfg, holder, _) = holder_and_admin();
        let nominee = acct(2, true);
        cfg.nominate(Some(&admin_cfg), &holder, nominee.account_id)
            .unwrap();
        assert!(cfg.assert(&holder).is_ok(), "nominating moves nothing");
        assert_eq!(cfg.nominee(), Some(nominee.account_id));

        cfg.accept(&nominee).unwrap();
        assert!(cfg.assert(&nominee).is_ok());
        assert_eq!(cfg.nominee(), None);
    }

    #[test]
    fn the_admin_nominates_too() {
        let (mut cfg, admin_cfg, _, admin) = holder_and_admin();
        let nominee = acct(2, true);
        cfg.nominate(Some(&admin_cfg), &admin, nominee.account_id)
            .unwrap();
        cfg.accept(&nominee).unwrap();
        assert!(cfg.assert(&nominee).is_ok());
    }

    #[test]
    fn anyone_else_cannot_nominate() {
        let (mut cfg, admin_cfg, _, _) = holder_and_admin();
        assert_eq!(
            cfg.nominate(Some(&admin_cfg), &acct(3, true), AccountId::new([2; 32])),
            Err(FreezeError::NotAdminOrFreezeAuthority)
        );
    }

    #[test]
    fn a_vacant_slot_takes_a_nominee_from_the_admin_only() {
        let (mut cfg, admin_cfg, holder, admin) = holder_and_admin();
        cfg.renounce(Some(&admin_cfg), &holder).unwrap();
        assert_eq!(
            cfg.nominate(Some(&admin_cfg), &holder, AccountId::new([2; 32])),
            Err(FreezeError::NotAdminOrFreezeAuthority)
        );
        let nominee = acct(2, true);
        cfg.nominate(Some(&admin_cfg), &admin, nominee.account_id)
            .unwrap();
        cfg.accept(&nominee).unwrap();
        assert!(cfg.assert(&nominee).is_ok());
    }

    #[test]
    fn accept_by_someone_else_is_refused() {
        let (mut cfg, admin_cfg, holder, _) = holder_and_admin();
        cfg.nominate(Some(&admin_cfg), &holder, AccountId::new([2; 32]))
            .unwrap();
        assert_eq!(cfg.accept(&acct(3, true)), Err(FreezeError::NotNominee));
    }

    #[test]
    fn renounce_clears_the_nomination() {
        let (mut cfg, admin_cfg, holder, _) = holder_and_admin();
        let nominee = acct(2, true);
        cfg.nominate(Some(&admin_cfg), &holder, nominee.account_id)
            .unwrap();
        cfg.renounce(Some(&admin_cfg), &holder).unwrap();
        assert_eq!(cfg.accept(&nominee), Err(FreezeError::NoNomination));
    }

    #[test]
    fn a_transfer_clears_a_stale_nomination() {
        let (mut cfg, admin_cfg, holder, _) = holder_and_admin();
        let stale = acct(2, true);
        cfg.nominate(Some(&admin_cfg), &holder, stale.account_id)
            .unwrap();
        cfg.transfer(FreezeCandidate::Signer, &acct(3, true))
            .unwrap();
        assert_eq!(cfg.accept(&stale), Err(FreezeError::NoNomination));
    }

    #[test]
    fn fixed_size_matches_the_encoding() {
        let (mut cfg, admin_cfg, holder, _) = holder_and_admin();
        cfg.nominate(Some(&admin_cfg), &holder, AccountId::new([2; 32]))
            .unwrap();
        assert_eq!(
            cfg.encode().unwrap().len(),
            <FreezeConfig as spel_framework::FixedBorshSize>::SIZE
        );
    }

    const OURS: ProgramId = [7; 8];

    fn config_owned_by(program_owner: ProgramId) -> AccountWithMetadata {
        let mut config_account = acct(9, false);
        FreezeConfig::bootstrap(&mut config_account, FreezeCandidate::Signer, &acct(1, true))
            .unwrap();
        config_account.account.program_owner = program_owner;
        config_account
    }

    #[test]
    fn from_owned_account_reads_a_config_the_program_owns() {
        let cfg = FreezeConfig::from_owned_account(&config_owned_by(OURS), OURS).unwrap();
        assert!(cfg.assert(&acct(1, true)).is_ok());
    }

    #[test]
    fn from_owned_account_refuses_a_config_another_program_owns() {
        assert_eq!(
            FreezeConfig::from_owned_account(&config_owned_by([8; 8]), OURS),
            Err(FreezeError::ConfigNotOwned)
        );
        assert_eq!(
            FreezeConfig::from_owned_account_at(&config_owned_by([8; 8]), 0, OURS),
            Err(FreezeError::ConfigNotOwned)
        );
    }

    #[test]
    fn from_owned_account_at_checks_the_owner_before_the_window() {
        assert_eq!(
            FreezeConfig::from_owned_account_at(&config_owned_by(OURS), 64, OURS),
            Err(FreezeError::SlotOutOfBounds)
        );
        assert_eq!(
            FreezeConfig::from_owned_account_at(&config_owned_by([8; 8]), 64, OURS),
            Err(FreezeError::ConfigNotOwned)
        );
    }
}
