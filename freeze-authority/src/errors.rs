#![warn(missing_docs)]

use admin_authority::AdminError;
use authority::AuthorityError;
use spel_framework::prelude::*;

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
            FreezeError::AccountDataTooLarge => {
                write!(f, "FreezeConfig too large for account data")
            }
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

#[cfg(test)]
mod test {
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
}
