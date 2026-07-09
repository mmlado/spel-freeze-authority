//! Freeze authority primitive for LEZ programs. Tracks the current freeze
//! authority and a program-wide frozen flag in a Config PDA, and tracks a
//! per-account frozen flag in per-target PDAs. The library exposes seven
//! management instructions and consumes the `#[freeze_authority]`,
//! `#[require_not_frozen]`, and `#[freeze_exempt]` macros.

use authority::{AuthoritySlot, AuthorityCandidate, AuthorityError};
use admin_authority::{AdminConfig, AdminError, require_admin};
use borsh::{BorshDeserialize, BorshSerialize};
use spel_framework::prelude::*;

pub use freeze_authority_macros::{
    freeze_authority, freeze_exempt, instruction, require_not_frozen,
};

extern crate self as freeze_authority;

pub type FreezeCandidate = AuthorityCandidate;

/// On-chain freeze authority state for a single program.
///
/// Stored in the program's Config PDA at `(program_id, "freeze_config")`.
/// Created once via `freeze_initialize`; cannot be reinitialized.
/// `freeze_authority == AccountId::default()` indicates the renounced state,
/// which per ADR-0007 is recoverable by admin via `freeze_authority_transfer`.
#[account_type]
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq)]
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
    pub fn initialize(freeze_authority: AccountId) -> Result<Self, FreezeError> {
        Ok(Self { 
            slot: AuthoritySlot::initialize(freeze_authority)?,
            is_frozen: false
        })
    }

    pub fn assert(&self, signer: &AccountWithMetadata) -> Result<(), FreezeError> {
        self.slot.assert(signer).map_err(FreezeError::from)
    }

    /// Borsh-serialises the config for storage in the PDA's account data.
    ///
    /// Returns `AdminError::EncodingFailed` if serialisation fails.
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

    /// Loads config from an account's data field. Convenience wrapper over
    /// [`FreezeConfig::decode`].
    pub fn from_account(account: &AccountWithMetadata) -> Result<Self, FreezeError> {
        Self::decode(&account.account.data)
    }

    /// Sets `is_frozen` to true locking down all functions that require not_freeze.
    ///
    /// Only the current freeze authority may call (`assert_admin` runs first).
    pub fn set_is_frozen(&mut self, current: &AccountWithMetadata, state: bool) -> Result<(), FreezeError> {
        self.slot.assert(current)?;
        self.is_frozen = state;
        Ok(())
    }

    /// Serialises and writes this config into an account's data field.
    ///
    /// Returns `AdminError:AccountDataTooLarge` if the encoded bytes exceed
    /// the account's max length.
    pub fn write_to(&self, account: &mut AccountWithMetadata) -> Result<(), FreezeError> {
        let bytes = self.encode()?;
        account.account.data = bytes
            .try_into()
            .map_err(|_| FreezeError::AccountDataTooLarge)?;
        Ok(())
    }

    /// Validates a candidate, builds a fresh config, and writes it to the PDA.
    ///
    /// Used by `freeze_initialize` and by consumers doing single-tx deploy +
    /// admin setup inside their own `initialize` handler.
    pub fn bootstrap(
        admin_account: &AccountWithMetadata,
        current: &AccountWithMetadata,
        config_account: &mut AccountWithMetadata,
        new_admin: FreezeCandidate,
        new_admin_account: &AccountWithMetadata,
    ) -> Result<(), FreezeError> {
        let admin_config= AdminConfig::from_account(admin_account)?;
        admin_config.assert_admin(current)?;
        let resolved = new_admin.validate(new_admin_account)?;
        let state = Self::initialize(resolved)?;
        state.write_to(config_account)
    }

    /// Replaces the current admin after authorising the caller and validating
    /// the incoming admin.
    ///
    /// Order is the security model: the caller must be the current admin
    /// (`assert_admin`) and the candidate must be valid
    /// (`validate_with_account`) before `self.admin` is overwritten. Either
    /// check failing leaves state untouched.
    pub fn transfer(
        &mut self,
        admin_account: &AccountWithMetadata,
        current: &AccountWithMetadata,
        candidate: FreezeCandidate,
        new_account: &AccountWithMetadata,
    ) -> Result<(), FreezeError> {
        let admin_config= AdminConfig::from_account(admin_account)?;
        admin_config.assert_admin(current)?;
        let next = candidate.validate(new_account)?;
        self.slot.transfer_to(next);
        Ok(())
    }

    /// Zeros the freeze authority to `AccountId::default()`, the renounced
    /// sentinel.
    ///
    /// Only the current freeze authority may call.
    pub fn renounce(
        &mut self,
        admin_account: &AccountWithMetadata,
        current: &AccountWithMetadata
    ) -> Result<(), FreezeError> {
        let admin_config= AdminConfig::from_account(admin_account)?;
        admin_config
            .assert_admin(current)
            .or_else(|_| self.slot.assert(current))?;
        self.slot.renounce();
        Ok(())
    }

    /// Loads config from account, renounce admin, writes back.
    pub fn perform_renounce(
        admin_account: &AccountWithMetadata,
        config_account: &mut AccountWithMetadata,
        current: &AccountWithMetadata,
    ) -> Result<(), FreezeError> {
        let mut state = Self::from_account(config_account)?;
        state.renounce(admin_account, current)?;
        state.write_to(config_account)
    }

    /// Loads config from account, freeze, writes back.
    pub fn perform_freeze(
        config_account: &mut AccountWithMetadata,
        current: &AccountWithMetadata,
    ) -> Result<(), FreezeError> {
        let mut state = Self::from_account(config_account)?;
        state.set_is_frozen(current, true)?;
        state.write_to(config_account)
    }

    /// Loads config from account, release, writes back.
    pub fn perform_release(
        config_account: &mut AccountWithMetadata,
        current: &AccountWithMetadata,
    ) -> Result<(), FreezeError> {
        let mut state = Self::from_account(config_account)?;
        state.set_is_frozen(current, false)?;
        state.write_to(config_account)
    }
}

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
    /// Decode from account data, treating empty bytes as never-frozen.
    /// Empty -> `Default::default()` (is_frozen=false) "reads-or-defaults"
    /// semantic for lazily-created per-account PDAs.
    /// Malformed non-empty bytes -> `FreezeError:DecodingFailed`.
    pub fn from_data_or_default(data: &[u8]) -> Result<Self, FreezeError> {
        if data.is_empty() {
            return Ok(Self::default());
        }
        borsh::from_slice(data).map_err(|_| FreezeError::DecodingFailed)
    }

    /// Loads config from an account's data field. Convenience wrapper over
    /// [`FreezeAccountState::from_data_or_default`].
    pub fn from_account(account: &AccountWithMetadata) -> Result<Self, FreezeError> {
        Self::from_data_or_default(&account.account.data)
    }

    /// Borsh-serialises the config for storage in the PDA's account data.
    ///
    /// Returns `AdminError::EncodingFailed` if serialisation fails.
    pub fn encode(&self) -> Result<Vec<u8>, FreezeError> {
        borsh::to_vec(self).map_err(|_| FreezeError::EncodingFailed)
    }

    /// Serialises and writes this config into an account's data field.
    ///
    /// Returns `AdminError:AccountDataTooLarge` if the encoded bytes exceed
    /// the account's max length.
    pub fn write_to(&self, account: &mut AccountWithMetadata) -> Result<(), FreezeError> {
        let bytes = self.encode()?;
        account.account.data = bytes
            .try_into()
            .map_err(|_| FreezeError::AccountDataTooLarge)?;
        Ok(())
    }

    /// Sets `is_frozen` to true locking down all functions that require not_freeze.
    ///
    /// Only the current freeze authority may call (`assert_admin` runs first).
    pub fn set_is_frozen(
        &mut self,
        freeze_config: &AccountWithMetadata,
        caller: &AccountWithMetadata,
        state: bool
    ) -> Result<(), FreezeError> {
        let authority_state = FreezeConfig::from_account(freeze_config)?;
        authority_state.assert(caller)?;
        self.is_frozen = state;
        Ok(())
    }

    /// Loads config from account, freeze account, write back.
    pub fn perform_freeze(
        freeze_config: &AccountWithMetadata,
        config_account: &mut AccountWithMetadata,
        current: &AccountWithMetadata,
    ) -> Result<(), FreezeError> {
        let mut state = Self::from_account(&config_account)?;
        state.set_is_frozen(freeze_config, current, true)?;
        state.write_to(config_account)
    }

    pub fn perform_release(
        freeze_config: &AccountWithMetadata,
        config_account: &mut AccountWithMetadata,
        current: &AccountWithMetadata,
    ) -> Result<(), FreezeError> {
        let mut state = Self::from_account(&config_account)?;
        state.set_is_frozen(freeze_config, current, false)?;
        state.write_to(config_account)
    }
}

/// Errors returned by `freeze-authority` library methods. Mapped to
/// `SpelError::Unauthorized` at the SPEL boundary so the lib stays
/// independent of the framework's error surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FreezeError {
    /// Config PDA data is empty; `freeze_initialize` has not been called.
    NotInitialized,
    /// `freeze_initialize` called on an already-initialised Config PDA.
    AlreadyInitialized,
    /// `FreezeCandidate::Signer` paired with a default `AccountId`.
    InvalidCandidate,
    /// `FreezeCandidate::Pda` references an undeployed PDA.
    UndeployedPda,
    /// Candidate's derived address does not match `new_freeze_account.account_id`.
    CandidateMismatch,
    /// Signer's `account_id` does not match the stored `freeze_authority`.
    NotFreezeAuthority,
    /// Signer's `account_id` does not match the admin (admin-authorized paths only).
    NotAdmin,
    NotAdminOrFreezeAuthority,
    /// Signer is not authorized (no valid signature in the WitnessSet).
    MissingSignature,
    /// Stored `freeze_authority` is `AccountId::default()`; slot is vacant.
    Renounced,
    /// `freeze_program` called while `is_frozen` is already `true`.
    AlreadyFrozen,
    /// `freeze_program_release` called while `is_frozen` is already `false`.
    NotFrozen,
    /// `freeze_account(target)` called while target's PDA already stores `true`.
    AccountAlreadyFrozen,
    /// `freeze_account_release(target)` called while target's PDA is absent
    /// or stores `false`.
    AccountNotFrozen,
    /// Borsh encoding of `FreezeConfig` failed.
    EncodingFailed,
    /// Borsh decoding of `FreezeConfig` failed.
    DecodingFailed,
    /// Program frozen
    Frozen,
    /// Account frozen
    AccountFrozen,
    /// Error in writing data
    AccountDataTooLarge,
}

impl core::fmt::Display for FreezeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FreezeError::NotInitialized => write!(f, "freeze authority not initialized"),
            FreezeError::AlreadyInitialized => write!(f, "freeze authority already initialized"),
            FreezeError::InvalidCandidate => write!(f, "invalid freeze candidate"),
            FreezeError::UndeployedPda => write!(f, "candidate PDA is not deployed"),
            FreezeError::CandidateMismatch => write!(f, "candidate address mismatch"),
            FreezeError::NotFreezeAuthority => {
                write!(f, "signer is not the current freeze authority")
            }
            FreezeError::NotAdmin => write!(f, "signer is not the current admin"),
            FreezeError::NotAdminOrFreezeAuthority => {
                write!(f, "signer is not the current admin or freeze authority")
            }
            FreezeError::MissingSignature => write!(f, "freeze signature missing"),
            FreezeError::Renounced => write!(f, "freeze authority renounced"),
            FreezeError::AlreadyFrozen => write!(f, "program is already frozen"),
            FreezeError::NotFrozen => write!(f, "program is not frozen"),
            FreezeError::AccountAlreadyFrozen => write!(f, "account is already frozen"),
            FreezeError::AccountNotFrozen => write!(f, "account is not frozen"),
            FreezeError::EncodingFailed => write!(f, "failed to encode freeze account state"),
            FreezeError::DecodingFailed => write!(f, "failed to decode freeze account state"),
            FreezeError::Frozen => write!(f, "program is frozen"),
            FreezeError::AccountFrozen => write!(f, "account is frozen"),
            FreezeError::AccountDataTooLarge => write!(f, "FreezeConfig too large for account data"),
        }
    }
}

impl From<FreezeError> for SpelError {
    fn from(e: FreezeError) -> Self {
        SpelError::Unauthorized {
            message: e.to_string(),
        }
    }
}

impl From<AuthorityError> for FreezeError {
    fn from(e: AuthorityError) -> Self {
        match e {
            AuthorityError::InvalidCandidate    => FreezeError::InvalidCandidate,
            AuthorityError::UndeployedPda       => FreezeError::UndeployedPda,
            AuthorityError::CandidateMismatch   => FreezeError::CandidateMismatch,
            AuthorityError::NotHolder           => FreezeError::NotAdmin,
            AuthorityError::Renounced           => FreezeError::Renounced,
            AuthorityError::MissingSignature    => FreezeError::MissingSignature,
        }
    }
}

impl From<AdminError> for FreezeError {
    fn from(e: AdminError) -> Self {
        match e {
            AdminError::NotInitialized  => FreezeError::NotInitialized,
            AdminError::DecodingFailed  => FreezeError::DecodingFailed,
            _                           => FreezeError::NotAdmin,
        }
    }
}

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
    FreezeConfig::bootstrap(&admin_config, &caller, &mut freeze_config, FreezeCandidate::Signer, &caller)?;
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

/// Sets the program-wide frozen flag to `true`.
///
/// Only the current freeze authority can call. While `is_frozen` is `true`,
/// every dispatched instruction except the F3 carve-outs and admin operations
/// rejects via the auto-wrap framework hook.
#[instruction]
pub fn freeze_program(
    #[account(mut, pda = literal("freeze_config"))] mut freeze_config: AccountWithMetadata,
    #[account(signer)] caller: AccountWithMetadata,
) -> SpelResult {
    FreezeConfig::perform_freeze(&mut freeze_config, &caller)?;
    Ok(SpelOutput::execute(
        vec![freeze_config.account, caller.account],
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
) -> SpelResult {
    FreezeConfig::perform_release(&mut freeze_config, &caller)?;
    Ok(SpelOutput::execute(
        vec![freeze_config.account, caller.account],
        vec![],
    ))
}

/// Replaces the current freeze authority with a new signer or PDA.
///
/// Only the current admin can call (per RFP-002 F2). Accepts both Initialized
/// and Renounced starting states per ADR-0007 — admin may repopulate a vacant
/// slot. F3 carve-out: callable while the program is frozen.
#[require_admin]
#[instruction]
#[freeze_exempt]
pub fn freeze_authority_transfer(
    #[account(pda = literal("admin_config"))] admin_config: AccountWithMetadata,
    #[account(mut, pda = literal("freeze_config"))] mut freeze_config: AccountWithMetadata,
    #[account(signer)] caller: AccountWithMetadata,
    _new_freeze_account: AccountWithMetadata,
    _new_freeze_authority: ::freeze_authority::FreezeCandidate,
) -> SpelResult {
    FreezeConfig::perform_release(&mut freeze_config, &caller)?;
    Ok(SpelOutput::execute(
        vec![admin_config.account, freeze_config.account, caller.account],
        vec![],
    ))
}

/// Vacates the freeze authority slot.
///
/// Authorized by either the current admin (per RFP-002 F5) or the current
/// freeze authority self-renouncing (per ADR-0004 — keeps an exit available
/// even after admin renounce). Writes `AccountId::default()` to
/// `freeze_authority`. Per ADR-0007, NOT terminal: admin can repopulate the
/// slot later via `freeze_authority_transfer`. F3 carve-out: callable while
/// the program is frozen.
#[instruction]
#[freeze_exempt]
pub fn freeze_authority_renounce(
    #[account(pda = literal("admin_config"))] admin_config: AccountWithMetadata,
    #[account(mut, pda = literal("freeze_config"))] mut freeze_config: AccountWithMetadata,
    #[account(signer)] caller: AccountWithMetadata,
) -> SpelResult {
    FreezeConfig::perform_renounce(&admin_config, &mut freeze_config, &caller)?;    
    Ok(SpelOutput::execute(
        vec![admin_config.account, freeze_config.account, caller.account],
        vec![],
    ))
}

/// Sets the per-account frozen flag to `true` for `target`.
///
/// Only the current freeze authority can call. First call against a target
/// inits the per-account PDA; subsequent calls toggle the bool in place
/// (PDAs persist per ADR-0008; LEZ has no close primitive). F3 carve-out:
/// callable while the program-wide frozen flag is `true` — useful for
/// preparing per-account blocks before unfreezing the program.
#[instruction]
#[freeze_exempt]
pub fn freeze_account(
    #[account(pda = literal("freeze_config"))] freeze_config: AccountWithMetadata,
    #[account(mut, pda = [literal("frozen"), arg("target")])] mut frozen_pda: AccountWithMetadata,
    #[account(signer)] caller: AccountWithMetadata,
    target: [u8; 32],
) -> SpelResult {
    let _ = target;
    FrozenAccountState::perform_freeze(&freeze_config, &mut frozen_pda, &caller)?;
    Ok(SpelOutput::execute(
        vec![freeze_config.account, frozen_pda.account, caller.account],
        vec![],
    ))
}

/// Sets the per-account frozen flag to `false` for `target`.
///
/// Only the current freeze authority can call. Mutates the existing
/// per-target PDA; rejects with `AccountNotFrozen` if the target was never
/// frozen. F3 carve-out: callable while the program-wide frozen flag is `true`.
#[instruction]
#[freeze_exempt]
pub fn freeze_account_release(
    #[account(pda = literal("freeze_config"))] freeze_config: AccountWithMetadata,
    #[account(mut, pda = [literal("frozen"), arg("target")])] mut frozen_pda: AccountWithMetadata,
    #[account(signer)] caller: AccountWithMetadata,
    target: [u8; 32],
) -> SpelResult {
    let _ = target;
    FrozenAccountState::perform_release(&freeze_config, &mut frozen_pda, &caller)?;
    Ok(SpelOutput::execute(
        vec![freeze_config.account, frozen_pda.account, caller.account],
        vec![],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freeze_error_display_strings() {
        assert_eq!(
            FreezeError::NotInitialized.to_string(),
            "freeze authority not initialized"
        );
        assert_eq!(
            FreezeError::AlreadyInitialized.to_string(),
            "freeze authority already initialized"
        );
        assert_eq!(
            FreezeError::InvalidCandidate.to_string(),
            "invalid freeze candidate"
        );
        assert_eq!(
            FreezeError::UndeployedPda.to_string(),
            "candidate PDA is not deployed"
        );
        assert_eq!(
            FreezeError::CandidateMismatch.to_string(),
            "candidate address mismatch"
        );
        assert_eq!(
            FreezeError::NotFreezeAuthority.to_string(),
            "signer is not the current freeze authority"
        );
        assert_eq!(
            FreezeError::NotAdmin.to_string(),
            "signer is not the current admin"
        );
        assert_eq!(
            FreezeError::MissingSignature.to_string(),
            "freeze signature missing"
        );
        assert_eq!(
            FreezeError::Renounced.to_string(),
            "freeze authority renounced"
        );
        assert_eq!(
            FreezeError::AlreadyFrozen.to_string(),
            "program is already frozen"
        );
        assert_eq!(FreezeError::NotFrozen.to_string(), "program is not frozen");
        assert_eq!(
            FreezeError::AccountAlreadyFrozen.to_string(),
            "account is already frozen"
        );
        assert_eq!(
            FreezeError::AccountNotFrozen.to_string(),
            "account is not frozen"
        );
        assert_eq!(
            FreezeError::NotAdminOrFreezeAuthority.to_string(),
            "signer is not the current admin or freeze authority"
        );
        assert_eq!(
            FreezeError::EncodingFailed.to_string(),
            "failed to encode freeze account state"
        );
        assert_eq!(
            FreezeError::DecodingFailed.to_string(),
            "failed to decode freeze account state"
        );
        assert_eq!(FreezeError::Frozen.to_string(), "program is frozen");
        assert_eq!(FreezeError::AccountFrozen.to_string(), "account is frozen");
        assert_eq!(
            FreezeError::AccountDataTooLarge.to_string(),
            "FreezeConfig too large for account data"
        );
    }

    #[test]
    fn freeze_error_maps_to_unauthorized() {
        let spel: SpelError = FreezeError::NotAdmin.into();
        match spel {
            SpelError::Unauthorized { message } => {
                assert_eq!(message, "signer is not the current admin");
            }
            other => panic!("expected Unauthorized, got {other:?}"),
        }
    }

    #[test]
    fn freeze_error_renounced_maps_to_unauthorized_with_message() {
        let spel: SpelError = FreezeError::Renounced.into();
        match spel {
            SpelError::Unauthorized { message } => {
                assert_eq!(message, "freeze authority renounced");
            }
            other => panic!("expected Unauthorized, got {other:?}"),
        }
    }

    #[test]
    fn from_data_or_default_empty_yields_default_unfrozen() {
        let data: [u8; 0]  = [];
        let state = FrozenAccountState::from_data_or_default(&data)
            .expect("empty bytes must decode cleanly to default");
        assert!(!state.is_frozen);
    }

    #[test]
    fn from_data_or_default_decodes_valid_frozen() {
        let data: [u8; 1]  = [1; 1];
        let state = FrozenAccountState::from_data_or_default(&data)
            .expect("byte 1 must decode cleanly to true");
        assert!(state.is_frozen);
    }

    #[test]
    fn from_data_or_default_decodes_valid_unfrozen() {
        let data: [u8; 1]  = [0; 1];
        let state = FrozenAccountState::from_data_or_default(&data)
            .expect("byte 0 must decode cleanly to false");
        assert!(!state.is_frozen);
    }

    #[test]
    fn from_data_or_default_malformed_errors() {
        let data: [u8; 1]  = [9; 1];
        let err = FrozenAccountState::from_data_or_default(&data)
            .expect_err("malformed bytes must not decode cleanly");
        assert_eq!(err, FreezeError::DecodingFailed);
    }

    #[test]
    fn freeze_config_decode_empty_returns_not_initialized() {
        let account  = AccountWithMetadata { 
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
        let decoded  = FreezeConfig::from_account(&account).unwrap();
        assert_eq!(decoded, cfg);
    }

    #[test]
    fn freeze_config_decode_malformed_errors() {
        let account  = AccountWithMetadata {
            account: Account { 
                data: vec![0xff, 5].try_into().unwrap(),
                ..Account::default() 
            },
            is_authorized: false,
            account_id: AccountId::new([0; 32]),
        };
        let err  = FreezeConfig::from_account(&account).unwrap_err();
        assert_eq!(err, FreezeError::DecodingFailed);
    }
}
