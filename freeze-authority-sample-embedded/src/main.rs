use admin_authority::{AdminCandidate, AdminConfig};
use borsh::{BorshDeserialize, BorshSerialize};
use freeze_authority::{FreezeCandidate, FreezeConfig, freeze_exempt};
use spel_framework::prelude::*;

/// Both authority slots live inside this account: value 0..8,
/// padding 8..32, admin 32..64, freeze 64..97. Two extensions, one
/// consumer account, distinct windows.
#[account_type]
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug)]
pub struct ProgramConfig {
    pub value: u64,
    pub padding: [u8; 24],
    pub admin: AdminConfig,
    pub freeze: FreezeConfig,
}

impl ProgramConfig {
    fn from_account(account: &AccountWithMetadata) -> Result<Self, SpelError> {
        Self::try_from_slice(&account.account.data).map_err(|_| SpelError::SerializationError {
            message: "decoding failed".into(),
        })
    }

    fn write_to(&self, account: &mut AccountWithMetadata) -> Result<(), SpelError> {
        account.account.data = borsh::to_vec(self)
            .map_err(|_| SpelError::SerializationError {
                message: "encoding failed".into(),
            })?
            .try_into()
            .map_err(|_| SpelError::SerializationError {
                message: "data too large".into(),
            })?;
        Ok(())
    }
}

#[lez_program]
#[admin_authority(admin_config = config, offset = 32)]
#[freeze_authority(freeze_config = config, offset = 64)]
mod freeze_authority_sample_embedded {
    use admin_authority::require_admin;

    /// Creates the embedding account, bootstraps the admin slot, and
    /// leaves the freeze slot born vacant: the admin appoints the first
    /// holder via freeze_authority_transfer.
    #[instruction]
    pub fn initialize(
        #[account(init, pda = literal("program_config"))] mut config: AccountWithMetadata,
        #[account(signer)] signer: AccountWithMetadata,
    ) -> SpelResult {
        ProgramConfig {
            value: 0,
            padding: [0; 24],
            admin: AdminConfig::default(),
            freeze: FreezeConfig::default(),
        }
        .write_to(&mut config)?;
        AdminConfig::bootstrap_at(&mut config, 32, AdminCandidate::Signer, &signer)?;
        Ok(SpelOutput::execute(vec![config, signer], vec![]))
    }

    /// Gated. The embedding account is declared, injection skips it, and
    /// the gate reads the admin slot at offset 32 from this very param.
    #[require_admin]
    #[instruction]
    pub fn update_value(
        #[account(mut, pda = literal("program_config"))] mut config: AccountWithMetadata,
        new_value: u64,
    ) -> SpelResult {
        let mut state = ProgramConfig::from_account(&config)?;
        state.value = new_value;
        state.write_to(&mut config)?;
        Ok(SpelOutput::execute(vec![config], vec![]))
    }

    #[instruction]
    #[freeze_exempt]
    pub fn read_value(
        #[account(pda = literal("program_config"))] config: AccountWithMetadata,
    ) -> SpelResult {
        let _state = ProgramConfig::from_account(&config)?;
        Ok(SpelOutput::execute(vec![config], vec![]))
    }

    #[require_admin]
    #[instruction]
    #[freeze_exempt]
    pub fn withdraw(
        #[account(mut, pda = literal("program_config"))] mut config: AccountWithMetadata,
    ) -> SpelResult {
        let mut state = ProgramConfig::from_account(&config)?;
        state.value = 0;
        state.write_to(&mut config)?;
        Ok(SpelOutput::execute(vec![config], vec![]))
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

    /// The consumer's initialize, distilled: struct written whole, admin
    /// bootstrapped at 32, freeze slot left born vacant at 64.
    fn shared_account(admin_signer: &AccountWithMetadata) -> AccountWithMetadata {
        let mut config_account = acct(9, false);
        ProgramConfig {
            value: 7,
            padding: [0xAB; 24],
            admin: AdminConfig::default(),
            freeze: FreezeConfig::default(),
        }
        .write_to(&mut config_account)
        .expect("initial write");
        AdminConfig::bootstrap_at(&mut config_account, 32, AdminCandidate::Signer, admin_signer)
            .expect("bootstrap");
        config_account
    }

    // Windows are adjacent: an admin handover and a freeze appointment
    // must each leave the consumer fields AND the neighboring slot alone.
    #[test]
    fn value_survives_admin_transfer_and_freeze_appointment() {
        let admin = acct(1, true);
        let new_admin = acct(2, true);
        let holder = acct(3, true);
        let mut config_account = shared_account(&admin);

        FreezeConfig::perform_transfer_at(
            &mut config_account,
            64,
            FreezeCandidate::Signer,
            &holder,
        )
        .expect("appoint holder");
        AdminConfig::perform_transfer_at(
            &mut config_account,
            32,
            &admin,
            AdminCandidate::Signer,
            &new_admin,
        )
        .expect("admin transfer");

        let state = ProgramConfig::from_account(&config_account).expect("decode");
        assert_eq!(state.value, 7, "consumer value trampled");
        assert_eq!(state.padding, [0xAB; 24], "padding trampled");
        assert!(state.admin.assert_admin(&new_admin).is_ok());
        assert!(
            state.freeze.assert(&holder).is_ok(),
            "freeze slot trampled by the admin transfer next door"
        );
    }

    // Toggling the freeze flag must not touch the admin window at 32..64.
    #[test]
    fn freeze_toggle_preserves_admin_window() {
        let admin = acct(1, true);
        let holder = acct(3, true);
        let mut config_account = shared_account(&admin);
        FreezeConfig::perform_transfer_at(
            &mut config_account,
            64,
            FreezeCandidate::Signer,
            &holder,
        )
        .expect("appoint");
        FreezeConfig::perform_freeze_at(&mut config_account, 64, &holder).expect("freeze");

        let state = ProgramConfig::from_account(&config_account).expect("decode");
        assert!(state.freeze.is_frozen);
        assert!(
            state.admin.assert_admin(&admin).is_ok(),
            "admin window trampled by the freeze toggle"
        );
        assert_eq!(state.value, 7);
    }

    // Born vacant: the unbootstrapped freeze slot rejects everyone until
    // the admin appoints a holder via transfer, the same path that
    // repopulates a renounced slot.
    #[test]
    fn freeze_slot_is_born_vacant_until_admin_appoints() {
        let admin = acct(1, true);
        let anyone = acct(4, true);
        let mut config_account = shared_account(&admin);

        let freeze = FreezeConfig::from_account_at(&config_account, 64).expect("decode");
        assert!(freeze.assert(&anyone).is_err(), "born vacant slot must reject");

        FreezeConfig::perform_transfer_at(
            &mut config_account,
            64,
            FreezeCandidate::Signer,
            &anyone,
        )
        .expect("admin appoints");
        let freeze = FreezeConfig::from_account_at(&config_account, 64).expect("decode");
        assert!(freeze.assert(&anyone).is_ok());
    }

    // The dispatcher's shared-account call shape, hand-written: one
    // account cloned into both role positions, exactly one post-state
    // out for it. This is the generated match arm's contract.
    #[test]
    fn shared_account_renounce_emits_single_post_state() {
        let admin = acct(1, true);
        let holder = acct(3, true);
        let mut config_account = shared_account(&admin);
        FreezeConfig::perform_transfer_at(
            &mut config_account,
            64,
            FreezeCandidate::Signer,
            &holder,
        )
        .expect("appoint");

        let out = freeze_authority::freeze_authority_renounce(
            config_account.clone(),
            config_account,
            holder,
            64,
            32,
        )
        .expect("renounce");
        assert_eq!(
            out.post_states.len(),
            2,
            "shared account must collapse to one post-state"
        );
    }
}
