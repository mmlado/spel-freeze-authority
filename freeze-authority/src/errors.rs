//! The library error type and its conversions from the shared
//! `authority` and `admin-authority` errors and into `SpelError` at the
//! SPEL boundary.

#![warn(missing_docs)]

use admin_authority::AdminError;
use authority::AuthorityError;
use spel_framework::prelude::*;

/// Errors returned by `freeze-authority` methods. Each maps to
/// `SpelError::Custom` with a stable code from [`FreezeError::code`], which
/// LEZ reports as program error `6000 + code`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FreezeError {
    /// Config PDA data is empty; `freeze_initialize` has not been called.
    NotInitialized,
    /// Reserved, not raised. `#[account(init)]` refuses a second
    /// `freeze_initialize`.
    AlreadyInitialized,
    /// `FreezeCandidate::Signer` paired with a default `AccountId`.
    InvalidCandidate,
    /// `FreezeCandidate::Pda` references an undeployed PDA.
    UndeployedPda,
    /// Candidate's derived address does not match `new_account.account_id`.
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
    /// Reserved, not raised. The auto-wrap gate rejects `freeze_program`
    /// on a frozen program with `Frozen`.
    AlreadyFrozen,
    /// Reserved, not raised. Releasing an unfrozen program succeeds without a change.
    NotFrozen,
    /// Reserved, not raised. Freezing a frozen account succeeds without a change.
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
    /// An embedded-slot window `[offset..offset+65)` does not fit inside
    /// the account's data. Layout error: the declared offset and the
    /// account's actual size disagree.
    SlotOutOfBounds,
    /// A `Signer` transfer target is owned by no program. LEZ would
    /// strand it after one transaction. Run `wallet auth-transfer init` on
    /// the key first.
    UnownedSigner,
    /// `freeze_authority_accept` was called with no nominee.
    NoNomination,
    /// The signer of `freeze_authority_accept` is not the nominee.
    NotNominee,
    /// The config account is not owned by the program reading it.
    ConfigNotOwned,
}

impl FreezeError {
    /// The stable code of this error, in the 2xx block.
    pub const fn code(&self) -> u32 {
        match self {
            FreezeError::NotInitialized => 201,
            FreezeError::AlreadyInitialized => 202,
            FreezeError::InvalidCandidate => 203,
            FreezeError::UndeployedPda => 204,
            FreezeError::CandidateMismatch => 205,
            FreezeError::NotFreezeAuthority => 206,
            FreezeError::NotAdmin => 207,
            FreezeError::NotAdminOrFreezeAuthority => 208,
            FreezeError::MissingSignature => 209,
            FreezeError::Renounced => 210,
            FreezeError::AlreadyFrozen => 211,
            FreezeError::NotFrozen => 212,
            FreezeError::AccountAlreadyFrozen => 213,
            FreezeError::AccountNotFrozen => 214,
            FreezeError::EncodingFailed => 215,
            FreezeError::DecodingFailed => 216,
            FreezeError::Frozen => 217,
            FreezeError::AccountFrozen => 218,
            FreezeError::AccountDataTooLarge => 219,
            FreezeError::SlotOutOfBounds => 220,
            FreezeError::UnownedSigner => 221,
            FreezeError::NoNomination => 222,
            FreezeError::NotNominee => 223,
            FreezeError::ConfigNotOwned => 224,
        }
    }
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
            FreezeError::UnownedSigner => write!(f, "freeze authority key is owned by no program"),
            FreezeError::NoNomination => write!(f, "no freeze authority nominee to accept"),
            FreezeError::NotNominee => write!(f, "signer is not the freeze authority nominee"),
            FreezeError::ConfigNotOwned => write!(f, "freeze config is not owned by this program"),
        }
    }
}

impl From<FreezeError> for SpelError {
    fn from(e: FreezeError) -> Self {
        SpelError::custom(e.code(), e.to_string())
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
            AuthorityError::UnownedSigner => FreezeError::UnownedSigner,
            AuthorityError::NoNomination => FreezeError::NoNomination,
            AuthorityError::NotNominee => FreezeError::NotNominee,
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
            AdminError::UnownedSigner => FreezeError::UnownedSigner,
            AdminError::NoNomination => FreezeError::NoNomination,
            AdminError::NotNominee => FreezeError::NotNominee,
            AdminError::NotGenesisAdmin => FreezeError::NotAdmin,
            AdminError::ConfigNotOwned => FreezeError::ConfigNotOwned,
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

    // Codes are API: clients branch on them, so a renumbering must fail here.
    #[test]
    fn codes_are_stable() {
        use FreezeError::*;
        let pinned = [
            (NotInitialized, 201),
            (AlreadyInitialized, 202),
            (InvalidCandidate, 203),
            (UndeployedPda, 204),
            (CandidateMismatch, 205),
            (NotFreezeAuthority, 206),
            (NotAdmin, 207),
            (NotAdminOrFreezeAuthority, 208),
            (MissingSignature, 209),
            (Renounced, 210),
            (AlreadyFrozen, 211),
            (NotFrozen, 212),
            (AccountAlreadyFrozen, 213),
            (AccountNotFrozen, 214),
            (EncodingFailed, 215),
            (DecodingFailed, 216),
            (Frozen, 217),
            (AccountFrozen, 218),
            (AccountDataTooLarge, 219),
            (SlotOutOfBounds, 220),
            (UnownedSigner, 221),
            (NoNomination, 222),
            (NotNominee, 223),
            (ConfigNotOwned, 224),
        ];
        for (err, code) in pinned {
            assert_eq!(err.code(), code, "{err:?}");
        }
    }

    #[test]
    fn spel_error_carries_the_code_and_the_message() {
        let spel: SpelError = FreezeError::Frozen.into();
        assert_eq!(spel.error_code(), 6217);
        match spel {
            SpelError::Custom { code, message } => {
                assert_eq!(code, 217);
                assert_eq!(message, "program is frozen");
            }
            other => panic!("expected Custom, got {other:?}"),
        }
    }
}
