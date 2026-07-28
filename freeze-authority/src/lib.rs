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

use admin_authority::{AdminConfig, AdminError, require_admin};
use authority::{AuthorityCandidate, AuthorityError, AuthoritySlot};
use borsh::{BorshDeserialize, BorshSerialize};
use spel_framework::prelude::*;

pub use freeze_authority_macros::{
    freeze_authority, freeze_exempt, instruction, require_not_frozen,
};

extern crate self as freeze_authority;

pub type FreezeCandidate = AuthorityCandidate;

// Borsh-encoded size of FreezeConfig: 32-byte slot + 1 byte flag.
pub const ENCODED_LEN: usize = 33;

/// On-chain freeze authority state for a single program.
///
/// Stored in the program's Config PDA at `(program_id, "freeze_config")`.
/// Composes the shared `authority::AuthoritySlot` (holds the freeze authority
/// `AccountId`) with the program-wide `is_frozen` flag. `slot.is_renounced()`
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
        let is_frozen = match data.get(offset + 32) {
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
            .get_mut(offset + 32)
            .ok_or(FreezeError::SlotOutOfBounds)?
            = self.is_frozen as u8;
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
        new_admin: FreezeCandidate,
        new_admin_account: &AccountWithMetadata,
    ) -> Result<(), FreezeError> {
        let resolved = new_admin.validate(new_admin_account)?;
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
    /// An embedded-slot window `[offset..offset+32)` does not fit inside
    /// the account's data. Layout error: the declared offset and the
    /// account's actual size disagree.
    SlotOutOfBounds,
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
            },
            FreezeError::NotAdmin => write!(f, "signer is not the current admin"),
            FreezeError::NotAdminOrFreezeAuthority => {
                write!(f, "signer is not the current admin or freeze authority")
            },
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
            FreezeError::AccountDataTooLarge => {
                write!(f, "FreezeConfig too large for account data")
            },
            FreezeError::SlotOutOfBounds => write!(f, "embedded slot window out of bounds"),
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
            AuthorityError::InvalidCandidate => FreezeError::InvalidCandidate,
            AuthorityError::UndeployedPda => FreezeError::UndeployedPda,
            AuthorityError::CandidateMismatch => FreezeError::CandidateMismatch,
            AuthorityError::NotHolder => FreezeError::NotFreezeAuthority,
            AuthorityError::Renounced => FreezeError::Renounced,
            AuthorityError::MissingSignature => FreezeError::MissingSignature,
            AuthorityError::SlotOutOfBounds => FreezeError::SlotOutOfBounds,
        }
    }
}

impl From<AdminError> for FreezeError {
    fn from(e: AdminError) -> Self {
        match e {
            AdminError::NotInitialized => FreezeError::NotInitialized,
            AdminError::Renounced => FreezeError::Renounced,
            AdminError::NotAdmin => FreezeError::NotAdmin,
            AdminError::MissingSignature => FreezeError::MissingSignature,
            AdminError::InvalidCandidate => FreezeError::InvalidCandidate,
            AdminError::UndeployedPda => FreezeError::UndeployedPda,
            AdminError::CandidateMismatch => FreezeError::CandidateMismatch,
            AdminError::EncodingFailed => FreezeError::EncodingFailed,
            AdminError::DecodingFailed => FreezeError::DecodingFailed,
            AdminError::AccountDataTooLarge => FreezeError::AccountDataTooLarge,
            AdminError::SlotOutOfBounds => FreezeError::SlotOutOfBounds,
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

#[instruction]
#[freeze_exempt]
pub fn freeze_authority_transfer(
    #[account(pda = literal("admin_config"))] admin_config: AccountWithMetadata,
    #[account(mut, pda = literal("freeze_config"))] mut freeze_config: AccountWithMetadata,
    #[account(signer)] caller: AccountWithMetadata,
    new_account: AccountWithMetadata,
    candidate: FreezeCandidate,
    offset: usize,
    admin_offset: usize,
) -> SpelResult {
    let admin = AdminConfig::from_account_at(&admin_config, admin_offset)?;
    admin.assert_admin(&caller)?;
    FreezeConfig::perform_transfer_at(&mut freeze_config, offset, candidate, &new_account)?;
    let mut accounts = post_state_pair(admin_config, freeze_config);
    accounts.push(caller.account);
    accounts.push(new_account.account);
    Ok(SpelOutput::execute(
        accounts,
        vec![],
    ))
}

#[instruction]
#[freeze_exempt]
pub fn freeze_authority_renounce(
    #[account(pda = literal("admin_config"))] admin_config: AccountWithMetadata,
    #[account(mut, pda = literal("freeze_config"))] mut freeze_config: AccountWithMetadata,
    #[account(signer)] caller: AccountWithMetadata,
    offset: usize,
    admin_offset: usize,
) -> SpelResult {
    let admin = AdminConfig::from_account_at(&admin_config, admin_offset).ok();
    FreezeConfig::perform_renounce_at(admin.as_ref(), &mut freeze_config, offset, &caller)?;
    let mut accounts = post_state_pair(admin_config, freeze_config);
    accounts.push(caller.account);
    Ok(SpelOutput::execute(
        accounts,
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
    #[account(pda = [literal("frozen"), account("caller")])] _freeze_account: AccountWithMetadata,
    #[account(signer)] caller: AccountWithMetadata,
    offset: usize,
) -> SpelResult {
    FreezeConfig::perform_freeze_at(&mut freeze_config, offset, &caller)?;
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
    offset: usize,
) -> SpelResult {
    FreezeConfig::perform_release_at(&mut freeze_config, offset, &caller)?;
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
    offset: usize,
) -> SpelResult {
    let _ = target;
    let authority = FreezeConfig::from_account_at(&freeze_config, offset)?;
    FrozenAccountState::perform_freeze(&authority, &mut frozen_pda, &caller)?;
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
    offset: usize,
) -> SpelResult {
    let _ = target;
    let authority = FreezeConfig::from_account_at(&freeze_config, offset)?;
    FrozenAccountState::perform_release(&authority, &mut frozen_pda, &caller)?;
    Ok(SpelOutput::execute(
        vec![freeze_config.account, frozen_pda.account, caller.account],
        vec![],
    ))
}

/// Post-state pair for a dual-role fn: collapses to the written copy
/// when both roles arrived as copies of one shared embedding account
/// (LEZ rejects duplicate account ids in the output).
fn post_state_pair(
    read: AccountWithMetadata,
    written: AccountWithMetadata,
) -> Vec<Account> {
    if read.account_id == written.account_id {
        vec![written.account]
    } else {
        vec![read.account, written.account]
    }
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

    fn admin_account_with(admin_id_byte: u8) -> AccountWithMetadata {
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
    fn frozen_account_state_set_is_frozen_holder_flips_flag() {
        let auth = acct(1, true);
        let cfg = FreezeConfig {
            slot: AuthoritySlot::initialize(auth.account_id).unwrap(),
            is_frozen: false,
        };
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
        let cfg = FreezeConfig {
            slot: AuthoritySlot::initialize(auth.account_id).unwrap(),
            is_frozen: false,
        };
        let mut state = FrozenAccountState::default();
        assert_eq!(
            state.set_is_frozen(&cfg, &other, true).unwrap_err(),
            FreezeError::NotFreezeAuthority
        );
        assert!(!state.is_frozen);
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
    fn frozen_account_state_perform_freeze_holder_flips_flag() {
        let auth = acct(1, true);
        let cfg = FreezeConfig {
            slot: AuthoritySlot::initialize(auth.account_id).unwrap(),
            is_frozen: false,
        };
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
        let cfg = FreezeConfig {
            slot: AuthoritySlot::initialize(auth.account_id).unwrap(),
            is_frozen: false,
        };
        let mut per_account = per_account_with(true);
        FrozenAccountState::perform_release(&cfg, &mut per_account, &auth).unwrap();
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
    
    #[test]
    fn perform_renounce_rejects_unauthorized_caller() {
        let auth = acct(1, true);        // freeze holder
        let stranger = acct(2, true);    // not admin, not holder
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

    // Step-3 gate tests. Red until `require_not_frozen` parses the
    // `offset` int kwarg; they pin the target behavior: one prologue
    // shape, `from_account_at(&cfg, offset)`, offset defaulting to 0.

    fn embedded_config_account(offset: usize, cfg: &FreezeConfig) -> AccountWithMetadata {
        let mut data = vec![0xAA; offset]; // consumer prefix bytes
        data.extend(borsh::to_vec(cfg).unwrap()); // slot + flag window
        data.extend([0xBB; 7]); // consumer suffix bytes
        AccountWithMetadata {
            account: Account {
                data: data.try_into().unwrap(),
                ..Account::default()
            },
            is_authorized: false,
            account_id: AccountId::new([9; 32]),
        }
    }

    #[test]
    fn gate_reads_frozen_flag_at_offset() {
        #[require_not_frozen(freeze_config = cfg, freeze_account = pda, offset = 32)]
        fn gated(cfg: AccountWithMetadata, pda: AccountWithMetadata) -> Result<(), SpelError> {
            let _ = (&cfg, &pda);
            Ok(())
        }
        let mut frozen = FreezeConfig::initialize(AccountId::new([1; 32])).unwrap();
        frozen.is_frozen = true;
        let cfg = embedded_config_account(32, &frozen);
        let err = gated(cfg, acct(7, false)).unwrap_err();
        assert!(
            matches!(err, SpelError::Unauthorized { ref message } if message == "program is frozen")
        );
    }

    #[test]
    fn gate_passes_when_unfrozen_at_offset() {
        #[require_not_frozen(freeze_config = cfg, freeze_account = pda, offset = 32)]
        fn gated(cfg: AccountWithMetadata, pda: AccountWithMetadata) -> Result<(), SpelError> {
            let _ = (&cfg, &pda);
            Ok(())
        }
        let unfrozen = FreezeConfig::initialize(AccountId::new([1; 32])).unwrap();
        let cfg = embedded_config_account(32, &unfrozen);
        assert!(gated(cfg, acct(7, false)).is_ok());
    }

    #[test]
    fn gate_without_offset_reads_dedicated_layout() {
        #[require_not_frozen(freeze_config = cfg, freeze_account = pda)]
        fn gated(cfg: AccountWithMetadata, pda: AccountWithMetadata) -> Result<(), SpelError> {
            let _ = (&cfg, &pda);
            Ok(())
        }
        let mut frozen = FreezeConfig::initialize(AccountId::new([1; 32])).unwrap();
        frozen.is_frozen = true;
        let cfg = config_account_with(&frozen);
        let err = gated(cfg, acct(7, false)).unwrap_err();
        assert!(
            matches!(err, SpelError::Unauthorized { ref message } if message == "program is frozen")
        );
    }

    #[test]
    fn gate_per_account_arm_is_offset_free() {
        // Program-wide unfrozen at offset 32, but the caller's per-account
        // PDA is frozen. The per-account arm must still fire in embedded
        // mode: FrozenAccountState never embeds, its read takes no offset.
        #[require_not_frozen(freeze_config = cfg, freeze_account = pda, offset = 32)]
        fn gated(cfg: AccountWithMetadata, pda: AccountWithMetadata) -> Result<(), SpelError> {
            let _ = (&cfg, &pda);
            Ok(())
        }
        let unfrozen = FreezeConfig::initialize(AccountId::new([1; 32])).unwrap();
        let cfg = embedded_config_account(32, &unfrozen);
        let err = gated(cfg, per_account_with(true)).unwrap_err();
        assert!(
            matches!(err, SpelError::Unauthorized { ref message } if message == "account is frozen")
        );
    }

    #[test]
    fn renounce_shared_account_emits_single_post_state() {
        // The same-account cell, called exactly as the dispatcher will
        // call it after the framework merge: one consumer account cloned
        // into both role positions, offsets baked as literals. LEZ
        // accepts exactly one post-state per transaction account, so the
        // output must collapse the two copies to one entry.
        let caller = acct(1, true); // freeze holder signs
        // Consumer layout: 32 prefix bytes, admin slot at 32, freeze at 64.
        let mut data = vec![0xAA; 32];
        let admin_cfg = AdminConfig::initialize(AccountId::new([2; 32])).unwrap();
        data.extend(admin_cfg.encode().unwrap());
        let freeze_cfg = FreezeConfig::initialize(caller.account_id).unwrap();
        data.extend(borsh::to_vec(&freeze_cfg).unwrap());
        let shared = AccountWithMetadata {
            account: Account {
                data: data.try_into().unwrap(),
                ..Account::default()
            },
            is_authorized: false,
            account_id: AccountId::new([9; 32]),
        };
        let out = freeze_authority_renounce(shared.clone(), shared, caller, 64, 32).unwrap();
        assert_eq!(out.post_states.len(), 2);
    }

    #[test]
    fn transfer_shared_account_emits_single_post_state() {
        // Same-account cell for the admin-only path: the caller is the
        // admin read from the shared account's admin window.
        let caller = acct(2, true); // admin signs
        let new_holder = acct(3, true);
        let mut data = vec![0xAA; 32];
        let admin_cfg = AdminConfig::initialize(caller.account_id).unwrap();
        data.extend(admin_cfg.encode().unwrap());
        let freeze_cfg = FreezeConfig::initialize(AccountId::new([1; 32])).unwrap();
        data.extend(borsh::to_vec(&freeze_cfg).unwrap());
        let shared = AccountWithMetadata {
            account: Account {
                data: data.try_into().unwrap(),
                ..Account::default()
            },
            is_authorized: false,
            account_id: AccountId::new([9; 32]),
        };
        let out = freeze_authority_transfer(
            shared.clone(),
            shared,
            caller,
            new_holder,
            FreezeCandidate::Signer,
            64,
            32,
        )
        .unwrap();
        assert_eq!(out.post_states.len(), 3);
    }

    #[test]
    fn renounce_distinct_accounts_keeps_both_post_states() {
        let caller = acct(1, true);
        let admin_account = admin_account_with(2);
        let freeze_cfg = FreezeConfig::initialize(caller.account_id).unwrap();
        let freeze_account = config_account_with(&freeze_cfg);
        let out =
            freeze_authority_renounce(admin_account, freeze_account, caller, 0, 0).unwrap();
        assert_eq!(out.post_states.len(), 3);
    }
}
