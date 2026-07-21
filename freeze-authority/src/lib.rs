//! Freeze authority primitive for LEZ programs. Tracks the current freeze
//! authority and a program-wide frozen flag in a Config PDA, and tracks a
//! per-account frozen flag in per-target PDAs. The library exposes seven
//! management instructions and consumes the `#[freeze_authority]`,
//! `#[require_not_frozen]`, and `#[freeze_exempt]` macros.

use admin_authority::require_admin;
use borsh::{BorshDeserialize, BorshSerialize};
use spel_framework::prelude::*;

pub use freeze_authority_macros::{
    freeze_authority, freeze_exempt, instruction, require_not_frozen,
};

extern crate self as freeze_authority;

/// Transfer-time argument describing the intended new freeze authority.
///
/// Paired with `new_freeze_account: AccountWithMetadata` at every transfer.
/// `FreezeCandidate` is the claim, `AccountWithMetadata` is the chain-state
/// evidence. One without the other provides no security guarantee.
#[derive(
    BorshSerialize,
    BorshDeserialize,
    serde::Serialize,
    serde::Deserialize,
    Clone,
    Debug,
    PartialEq,
    Eq,
)]
pub enum FreezeCandidate {
    /// New freeze authority is a keyholder. Validated by checking
    /// `new_freeze_account.is_authorized == true` (co-signed the tx).
    Signer,
    /// New freeze authority is a program-owned PDA. Validated by deriving
    /// the address from `(program_id, seed)`, matching it against
    /// `new_freeze_account`, and confirming the PDA is initialized.
    Pda {
        program_id: AccountId,
        seed: [u8; 32],
    },
}

/// On-chain freeze authority state for a single program.
///
/// Stored in the program's Config PDA at `(program_id, "freeze_config")`.
/// Created once via `freeze_initialize`; cannot be reinitialized.
/// `freeze_authority == AccountId::default()` indicates the renounced state,
/// which per ADR-0007 is recoverable by admin via `freeze_authority_transfer`.
#[account_type]
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct FreezeConfig {
    /// Current freeze authority's `AccountId`. `AccountId::default()` means
    /// the slot is vacant (Renounced); admin may repopulate.
    pub freeze_authority: AccountId,
    /// Program-wide frozen flag. Toggled by `freeze_program` /
    /// `freeze_program_release`.
    pub is_frozen: bool,
}

/// Per-account freeze state.
///
/// Stored in a per-target PDA at `(program_id, "frozen", target)`. Created
/// on first `freeze_account(target)`; mutated in place on subsequent
/// `freeze_account` / `freeze_account_release` calls. Per ADR-0008, PDAs
/// persist for their lifetime (LEZ has no close primitive).
#[account_type]
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct FrozenAccountState {
    /// Whether the target `AccountId` is currently frozen for this program.
    pub is_frozen: bool,
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
            FreezeError::MissingSignature => write!(f, "freeze signature missing"),
            FreezeError::Renounced => write!(f, "freeze authority renounced"),
            FreezeError::AlreadyFrozen => write!(f, "program is already frozen"),
            FreezeError::NotFrozen => write!(f, "program is not frozen"),
            FreezeError::AccountAlreadyFrozen => write!(f, "account is already frozen"),
            FreezeError::AccountNotFrozen => write!(f, "account is not frozen"),
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
#[require_admin] // In front of instruction for local build
#[instruction]
#[freeze_exempt]
pub fn freeze_initialize(
    #[account(init, pda = literal("freeze_config"))] mut freeze_config: AccountWithMetadata,
    #[account(pda = literal("admin_config"))] admin_config: AccountWithMetadata,
    #[account(signer)] signer: AccountWithMetadata,
    new_freeze_account: AccountWithMetadata,
    new_freeze_authority: ::freeze_authority::FreezeCandidate,
) -> SpelResult {
    todo!()
}

/// Sets the program-wide frozen flag to `true`.
///
/// Only the current freeze authority can call. While `is_frozen` is `true`,
/// every dispatched instruction except the F3 carve-outs and admin operations
/// rejects via the auto-wrap framework hook.
#[instruction]
pub fn freeze_program(
    #[account(mut, pda = literal("freeze_config"))] mut freeze_config: AccountWithMetadata,
    #[account(signer)] freeze_signer: AccountWithMetadata,
) -> SpelResult {
    todo!()
}

/// Sets the program-wide frozen flag to `false`.
///
/// Only the current freeze authority can call. F3 carve-out: callable while
/// the program is frozen.
#[instruction]
#[freeze_exempt]
pub fn freeze_program_release(
    #[account(mut, pda = literal("freeze_config"))] mut freeze_config: AccountWithMetadata,
    #[account(signer)] freeze_signer: AccountWithMetadata,
) -> SpelResult {
    todo!()
}

/// Replaces the current freeze authority with a new signer or PDA.
///
/// Only the current admin can call (per RFP-002 F2). Accepts both Initialized
/// and Renounced starting states per ADR-0007 — admin may repopulate a vacant
/// slot. F3 carve-out: callable while the program is frozen.
#[require_admin] // In front of instruction for local build
#[instruction]
#[freeze_exempt]
pub fn freeze_authority_transfer(
    #[account(mut, pda = literal("freeze_config"))] mut freeze_config: AccountWithMetadata,
    #[account(pda = literal("admin_config"))] admin_config: AccountWithMetadata,
    #[account(signer)] signer: AccountWithMetadata,
    new_freeze_account: AccountWithMetadata,
    new_freeze_authority: ::freeze_authority::FreezeCandidate,
) -> SpelResult {
    todo!()
}

/// Vacates the freeze authority slot.
///
/// Authorized by either the current admin (per RFP-002 F5) or the current
/// freeze authority self-renouncing (per ADR-0004 — keeps an exit available
/// even after admin renounce). Writes `AccountId::default()` to
/// `freeze_authority`. Per ADR-0007, NOT terminal: admin can repopulate the
/// slot later via `freeze_authority_transfer`. F3 carve-out: callable while
/// the program is frozen.
#[require_admin] // In front of instruction for local build
#[instruction]
#[freeze_exempt]
pub fn freeze_authority_renounce(
    #[account(mut, pda = literal("freeze_config"))] mut freeze_config: AccountWithMetadata,
    #[account(pda = literal("admin_config"))] admin_config: AccountWithMetadata,
    #[account(signer)] signer: AccountWithMetadata,
) -> SpelResult {
    todo!()
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
    #[account(init, pda = [literal("frozen"), arg("target")])] mut frozen_pda: AccountWithMetadata,
    #[account(mut, pda = literal("freeze_config"))] freeze_config: AccountWithMetadata,
    #[account(signer)] freeze_signer: AccountWithMetadata,
    target: [u8; 32],
) -> SpelResult {
    todo!()
}

/// Sets the per-account frozen flag to `false` for `target`.
///
/// Only the current freeze authority can call. Mutates the existing
/// per-target PDA; rejects with `AccountNotFrozen` if the target was never
/// frozen. F3 carve-out: callable while the program-wide frozen flag is `true`.
#[instruction]
#[freeze_exempt]
pub fn freeze_account_release(
    #[account(mut, pda = [literal("frozen"), arg("target")])] mut frozen_pda: AccountWithMetadata,
    #[account(mut, pda = literal("freeze_config"))] freeze_config: AccountWithMetadata,
    #[account(signer)] freeze_signer: AccountWithMetadata,
    target: [u8; 32],
) -> SpelResult {
    todo!()
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
}
