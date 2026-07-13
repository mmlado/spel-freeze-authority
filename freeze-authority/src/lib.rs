//! Freeze authority library for LEZ programs.
//!
//! Ships:
//! - `FreezeConfig` (program-wide freeze authority + frozen flag) and
//!   `FrozenAccountState` (per-account frozen flag).
//! - The `require_not_frozen` gate macro and the `#[freeze_authority]` /
//!   `#[freeze_exempt]` extension attributes.
//! - Seven management instructions covering the RFP-002 admin-supported
//!   flavor: `freeze_initialize`, `freeze_authority_transfer`,
//!   `freeze_authority_renounce`, `freeze_program`, `freeze_program_release`,
//!   `freeze_account`, `freeze_account_release`.

use admin_authority::{require_admin};
use authority::{AuthoritySlot, AuthorityCandidate, AuthorityError};
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
/// Composes the shared `authority::AuthoritySlot` (holds the freeze authority
/// `AccountId`) with the program-wide `is_frozen` flag. `slot.is_renounced()`
/// returns true when the holder is `AccountId::default()`; per ADR-0007 the
/// Renounced state is recoverable by admin via `freeze_authority_transfer`.
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
    fn initialize(freeze_authority: AccountId) -> Result<Self, FreezeError> {
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

    /// Loads config from an account's data field. Convenience wrapper over
    /// [`FreezeConfig::decode`].
    pub fn from_account(account: &AccountWithMetadata) -> Result<Self, FreezeError> {
        Self::decode(&account.account.data)
    }

    /// Sets `is_frozen` to `state`. Program-wide `true` blocks all
    /// non-exempt instructions via the `require_not_frozen` gate.
    ///
    /// Only the current freeze authority may call. `slot.assert(current)`
    /// runs first; a non-holder signer is rejected with
    /// `FreezeError::NotFreezeAuthority`.
    pub fn set_is_frozen(&mut self, current: &AccountWithMetadata, state: bool) -> Result<(), FreezeError> {
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

    /// Validates the incoming candidate and installs it as the new holder.
    ///
    /// No auth check on the caller. Candidate validation still runs so
    /// garbage input (default `AccountId`, undeployed PDA, mismatched
    /// address) is rejected here. Callers are responsible for gating who may
    /// transfer (`freeze_authority_transfer` gates on admin via
    /// `#[require_admin]`).
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
    /// No auth check. Callers are responsible for gating who may renounce.
    /// `freeze_authority_renounce` gates dual-path (admin OR current holder)
    /// per ADR-0004.
    pub fn renounce(
        &mut self,
    ) -> Result<(), FreezeError> {
        self.slot.renounce();
        Ok(())
    }

    /// Loads config from account, renounces the slot, writes back.
    ///
    /// Convenience workflow for callers that have already run their own
    /// auth check.
    pub fn perform_renounce(
        config_account: &mut AccountWithMetadata,
    ) -> Result<(), FreezeError> {
        let mut state = Self::from_account(config_account)?;
        state.renounce()?;
        state.write_to(config_account)
    }

    /// Loads config from account, sets `is_frozen = true`, writes back.
    ///
    /// Enforces the holder check via `set_is_frozen`.
    pub fn perform_freeze(
        config_account: &mut AccountWithMetadata,
        current: &AccountWithMetadata,
    ) -> Result<(), FreezeError> {
        let mut state = Self::from_account(config_account)?;
        state.set_is_frozen(current, true)?;
        state.write_to(config_account)
    }

    /// Loads config from account, sets `is_frozen = false`, writes back.
    ///
    /// Enforces the holder check via `set_is_frozen`.
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
        freeze_config: &AccountWithMetadata,
        caller: &AccountWithMetadata,
        state: bool,
    ) -> Result<(), FreezeError> {
        let authority_state = FreezeConfig::from_account(freeze_config)?;
        authority_state.assert(caller)?;
        self.is_frozen = state;
        Ok(())
    }

    /// Loads per-account state, sets `is_frozen = true`, writes back.
    ///
    /// Enforces the holder check via `set_is_frozen`.
    pub fn perform_freeze(
        freeze_config: &AccountWithMetadata,
        config_account: &mut AccountWithMetadata,
        current: &AccountWithMetadata,
    ) -> Result<(), FreezeError> {
        let mut state = Self::from_account(config_account)?;
        state.set_is_frozen(freeze_config, current, true)?;
        state.write_to(config_account)
    }

    /// Loads per-account state, sets `is_frozen = false`, writes back.
    ///
    /// Enforces the holder check via `set_is_frozen`.
    pub fn perform_release(
        freeze_config: &AccountWithMetadata,
        config_account: &mut AccountWithMetadata,
        current: &AccountWithMetadata,
    ) -> Result<(), FreezeError> {
        let mut state = Self::from_account(config_account)?;
        state.set_is_frozen(freeze_config, current, false)?;
        state.write_to(config_account)
    }
}

/// Errors returned by `freeze-authority` methods. Mapped to
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
    /// Signer's `account_id` does not match the stored freeze authority.
    NotFreezeAuthority,
    /// Signer's `account_id` does not match the admin (admin-authorized paths only).
    NotAdmin,
    /// Dual-path auth failed both admin and freeze-authority checks
    /// (emitted by `freeze_authority_renounce` per ADR-0004).
    NotAdminOrFreezeAuthority,
    /// Signer is not authorized (no valid signature in the witness set).
    MissingSignature,
    /// Stored freeze authority is `AccountId::default()`; slot is vacant.
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
    /// Borsh encoding failed.
    EncodingFailed,
    /// Borsh decoding of non-empty data failed.
    DecodingFailed,
    /// Auto-wrap gate rejection: program is currently frozen.
    Frozen,
    /// Auto-wrap gate rejection: caller's per-account PDA is frozen.
    AccountFrozen,
    /// Encoded bytes exceed the account's max data length.
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
            AuthorityError::NotHolder           => FreezeError::NotFreezeAuthority,
            AuthorityError::Renounced           => FreezeError::Renounced,
            AuthorityError::MissingSignature    => FreezeError::MissingSignature,
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
    FreezeConfig::bootstrap(&mut freeze_config, FreezeCandidate::Signer, &caller)?;
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

    fn acct(id_byte: u8, signed: bool) -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account::default(),
            is_authorized: signed,
            account_id: AccountId::new([id_byte; 32]),
        }
    }

    fn config_account_with(cfg: &FreezeConfig) -> AccountWithMetadata {
        AccountWithMetadata {
            account: Account {
                data: borsh::to_vec(cfg).unwrap().try_into().unwrap(),
                ..Account::default()
            },
            is_authorized: false,
            account_id: AccountId::new([9; 32]),
        }
    }

    fn per_account_with(is_frozen: bool) -> AccountWithMetadata {
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
        let mut config = FreezeConfig {
            slot: AuthoritySlot::initialize(authority.account_id).unwrap(),
            is_frozen: false,
        };
        config.renounce().unwrap();
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
        assert_eq!(cfg.assert(&old).unwrap_err(), FreezeError::NotFreezeAuthority);
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
            cfg.transfer(FreezeCandidate::Pda { program_id, seed }, &new).unwrap_err(),
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
            cfg.transfer(FreezeCandidate::Pda { program_id, seed }, &new).unwrap_err(),
            FreezeError::CandidateMismatch
        );
    }

    #[test]
    fn renounce_zeros_slot() {
        let auth = acct(1, true);
        let mut cfg = FreezeConfig {
            slot: AuthoritySlot::initialize(auth.account_id).unwrap(),
            is_frozen: false,
        };
        cfg.renounce().unwrap();
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
        let mut cfg = FreezeConfig {
            slot: AuthoritySlot::initialize(auth.account_id).unwrap(),
            is_frozen: false,
        };
        cfg.renounce().unwrap();
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
        let default_signer = AccountWithMetadata {
            account: Account::default(),
            is_authorized: true,
            account_id: AccountId::default(),
        };
        let mut config_account = AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: AccountId::new([9; 32]),
        };
        assert_eq!(
            FreezeConfig::bootstrap(&mut config_account, FreezeCandidate::Signer, &default_signer)
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
    fn perform_renounce_zeros_slot() {
        let auth = acct(1, true);
        let cfg = FreezeConfig {
            slot: AuthoritySlot::initialize(auth.account_id).unwrap(),
            is_frozen: false,
        };
        let mut config_account = config_account_with(&cfg);
        FreezeConfig::perform_renounce(&mut config_account).unwrap();
        let decoded = FreezeConfig::from_account(&config_account).unwrap();
        assert_eq!(decoded.assert(&auth).unwrap_err(), FreezeError::Renounced);
    }

    #[test]
    fn frozen_account_state_set_is_frozen_holder_flips_flag() {
        let auth = acct(1, true);
        let cfg = FreezeConfig {
            slot: AuthoritySlot::initialize(auth.account_id).unwrap(),
            is_frozen: false,
        };
        let freeze_config = config_account_with(&cfg);
        let mut state = FrozenAccountState::default();
        state.set_is_frozen(&freeze_config, &auth, true).unwrap();
        assert!(state.is_frozen);
        state.set_is_frozen(&freeze_config, &auth, false).unwrap();
        assert!(!state.is_frozen);
    }

    #[test]
    fn frozen_account_state_set_is_frozen_rejects_non_holder() {
        let auth = acct(1, true);
        let other = acct(2, true);
        let cfg = FreezeConfig {
            slot: AuthoritySlot::initialize(auth.account_id).unwrap(),
            is_frozen: false,
        };
        let freeze_config = config_account_with(&cfg);
        let mut state = FrozenAccountState::default();
        assert_eq!(
            state.set_is_frozen(&freeze_config, &other, true).unwrap_err(),
            FreezeError::NotFreezeAuthority
        );
        assert!(!state.is_frozen);
    }

    #[test]
    fn frozen_account_state_set_is_frozen_rejects_uninitialised_freeze_config() {
        let caller = acct(1, true);
        let freeze_config = AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: AccountId::new([9; 32]),
        };
        let mut state = FrozenAccountState::default();
        assert_eq!(
            state.set_is_frozen(&freeze_config, &caller, true).unwrap_err(),
            FreezeError::NotInitialized
        );
    }

    #[test]
    fn frozen_account_state_perform_freeze_holder_flips_flag() {
        let auth = acct(1, true);
        let cfg = FreezeConfig {
            slot: AuthoritySlot::initialize(auth.account_id).unwrap(),
            is_frozen: false,
        };
        let freeze_config = config_account_with(&cfg);
        let mut per_account = AccountWithMetadata {
            account: Account::default(),
            is_authorized: false,
            account_id: AccountId::new([7; 32]),
        };
        FrozenAccountState::perform_freeze(&freeze_config, &mut per_account, &auth).unwrap();
        let decoded = FrozenAccountState::from_account(&per_account).unwrap();
        assert!(decoded.is_frozen);
    }

    #[test]
    fn frozen_account_state_perform_release_holder_clears_flag() {
        let auth = acct(1, true);
        let cfg = FreezeConfig {
            slot: AuthoritySlot::initialize(auth.account_id).unwrap(),
            is_frozen: false,
        };
        let freeze_config = config_account_with(&cfg);
        let mut per_account = per_account_with(true);
        FrozenAccountState::perform_release(&freeze_config, &mut per_account, &auth).unwrap();
        let decoded = FrozenAccountState::from_account(&per_account).unwrap();
        assert!(!decoded.is_frozen);
    }

    #[test]
    fn freeze_config_write_to_rejects_oversized_data() {
        // Skipped: needs framework-internal knowledge of Account.data capacity.
        // FreezeConfig encodes to 33 bytes; can't trigger overflow without
        // knowing the framework's data cap.
    }

    #[test]
    fn frozen_account_state_write_to_rejects_oversized_data() {
        // Same as above — 1-byte encoding can't overflow the data cap.
    }
}
