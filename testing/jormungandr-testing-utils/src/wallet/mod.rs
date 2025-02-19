pub mod account;
pub mod committee;
pub mod delegation;
pub mod utxo;

pub use committee::{
    ElectionPublicKeyExtension, PrivateVoteCommitteeData, PrivateVoteCommitteeDataManager,
};

use crate::{
    qr_code::{generate, KeyQrCode},
    stake_pool::StakePool,
    testing::{FragmentBuilder, FragmentBuilderError},
};
use chain_addr::Discrimination;
use chain_crypto::{Ed25519, Signature};
pub use chain_impl_mockchain::{
    account::SpendingCounter,
    block::Block,
    certificate::{PoolId, SignedCertificate},
    chaintypes::ConsensusVersion,
    fee::LinearFee,
    fragment::Fragment,
    header::HeaderId,
    milli::Milli,
    transaction::{Input, TransactionBindingAuthData, UnspecifiedAccountIdentifier},
};
use chain_impl_mockchain::{
    block::BlockDate,
    certificate::{VotePlan, VoteTallyPayload},
    fee::FeeAlgorithm,
    key::EitherEd25519SecretKey,
    testing::data::{AddressData, AddressDataValue, Wallet as WalletLib},
    transaction::{
        InputOutputBuilder, Payload, PayloadSlice, TransactionBindingAuthDataPhantom,
        TransactionSignDataHash, Witness,
    },
    value::Value as ValueLib,
    vote::{Choice, CommitteeId},
};
use jormungandr_lib::{
    crypto::{account::Identifier as AccountIdentifier, hash::Hash, key::Identifier},
    interfaces::{Address, CommitteeIdDef, Initial, InitialUTxO, Value},
};
use rand_core::{CryptoRng, RngCore};
use std::io::Write;
use std::{fs::File, path::Path};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum WalletError {
    #[error("couldn't create file")]
    IoError(#[from] std::io::Error),
    #[error("cannot add input to the transaction")]
    CannotAddInput,
    #[error("cannot make witness for the transaction")]
    CannotMakeWitness,
    #[error("transaction error")]
    FragmentError(#[from] FragmentBuilderError),
    #[error("Invalid data")]
    InvalidBech32(#[from] bech32::Error),
    #[error("invalid electin public key")]
    ElectionPublicKey,
    #[error("invalid bech32 public key, expected {expected} hrp got {actual}")]
    InvalidBech32Key { expected: String, actual: String },
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum Wallet {
    Account(account::Wallet),
    UTxO(utxo::Wallet),
    Delegation(delegation::Wallet),
}

impl Wallet {
    pub fn new_account<RNG>(rng: &mut RNG) -> Wallet
    where
        RNG: CryptoRng + RngCore,
    {
        Self::new_account_with_discrimination(rng, Discrimination::Test)
    }

    pub fn import_account<P: AsRef<Path>>(
        secret_key_file: P,
        spending_counter: Option<u32>,
    ) -> Wallet {
        let bech32_str = jortestkit::file::read_file(secret_key_file);
        Wallet::Account(account::Wallet::from_existing_account(
            &bech32_str,
            spending_counter,
        ))
    }

    pub fn new_account_with_discrimination<RNG>(
        rng: &mut RNG,
        discrimination: Discrimination,
    ) -> Wallet
    where
        RNG: CryptoRng + RngCore,
    {
        Wallet::Account(account::Wallet::generate(rng, discrimination))
    }

    pub fn from_existing_account(
        signing_key_bech32: &str,
        spending_counter: Option<u32>,
    ) -> Wallet {
        Wallet::Account(account::Wallet::from_existing_account(
            signing_key_bech32,
            spending_counter,
        ))
    }

    pub fn to_initial_fund(&self, value: u64) -> InitialUTxO {
        InitialUTxO {
            address: self.address(),
            value: value.into(),
        }
    }

    pub fn new_utxo<RNG>(rng: &mut RNG) -> Wallet
    where
        RNG: CryptoRng + RngCore,
    {
        Self::new_utxo_with_discrimination(rng, Discrimination::Test)
    }

    pub fn new_utxo_with_discrimination<RNG>(
        rng: &mut RNG,
        discrimination: Discrimination,
    ) -> Wallet
    where
        RNG: CryptoRng + RngCore,
    {
        Wallet::UTxO(utxo::Wallet::generate(rng, discrimination))
    }

    pub fn new_delegation<RNG>(delegation_identifier: &AccountIdentifier, rng: &mut RNG) -> Wallet
    where
        RNG: CryptoRng + RngCore,
    {
        Self::new_delegation_with_discrimination(delegation_identifier, rng, Discrimination::Test)
    }

    pub fn new_delegation_with_discrimination<RNG>(
        delegation_identifier: &AccountIdentifier,
        rng: &mut RNG,
        discrimination: Discrimination,
    ) -> Wallet
    where
        RNG: CryptoRng + RngCore,
    {
        let mut delegation = delegation::Wallet::generate(rng, discrimination);
        delegation.generate_new_signing_key(delegation_identifier.clone());
        Wallet::Delegation(delegation)
    }

    pub fn save_qr_code<P: AsRef<Path>>(&self, path: P, password: &[u8]) {
        let qr = match self {
            Wallet::Account(account) => {
                let secret_key = match account.signing_key().as_ref() {
                    EitherEd25519SecretKey::Extended(secret_key) => secret_key,
                    EitherEd25519SecretKey::Normal(_) => panic!("unsupported secret key type"),
                };
                KeyQrCode::generate(secret_key.clone(), password)
            }
            Wallet::UTxO(utxo) => {
                KeyQrCode::generate(utxo.last_signing_key().clone().into_secret_key(), password)
            }
            Wallet::Delegation(delegation) => KeyQrCode::generate(
                delegation.last_signing_key().clone().into_secret_key(),
                password,
            ),
        };

        qr.to_img().save(path).unwrap();
    }

    pub fn save_qr_code_hash<P: AsRef<Path>>(
        &self,
        path: P,
        password: &[u8],
    ) -> std::io::Result<()> {
        let qr = match self {
            Wallet::Account(account) => {
                let secret_key = match account.signing_key().as_ref() {
                    EitherEd25519SecretKey::Extended(secret_key) => secret_key,
                    EitherEd25519SecretKey::Normal(_) => panic!("unsupported secret key type"),
                };
                generate(secret_key.clone(), password)
            }
            Wallet::UTxO(utxo) => {
                generate(utxo.last_signing_key().clone().into_secret_key(), password)
            }
            Wallet::Delegation(delegation) => generate(
                delegation.last_signing_key().clone().into_secret_key(),
                password,
            ),
        };

        let mut file = File::create(path).unwrap();
        writeln!(file, "{}", qr)
    }

    pub fn save_to_path<P: AsRef<Path>>(&self, path: P) -> std::io::Result<()> {
        let file = File::create(path).unwrap();
        self.save_to(&file)
    }

    pub fn save_to<W: std::io::Write>(&self, w: W) -> std::io::Result<()> {
        match self {
            Wallet::Account(account) => account.save_to(w),
            Wallet::UTxO(utxo) => utxo.save_to(w),
            _ => unimplemented!(),
        }
    }

    pub fn address(&self) -> Address {
        match self {
            Wallet::Account(account) => account.address(),
            Wallet::UTxO(utxo) => utxo.address(),
            Wallet::Delegation(delegation) => delegation.address(),
        }
    }

    pub fn sign_slice(&self, data: &[u8]) -> Signature<TransactionBindingAuthDataPhantom, Ed25519> {
        match self {
            Wallet::Account(account) => account.signing_key().as_ref().sign_slice(data),
            _ => unimplemented!(),
        }
    }

    /// Temporary method exposing private key
    pub fn signing_key_to_string(&self) -> String {
        match self {
            Wallet::Account(account) => account.signing_key().to_bech32_str(),
            Wallet::UTxO(utxo) => utxo.last_signing_key().to_bech32_str(),
            Wallet::Delegation(delegation) => delegation.last_signing_key().to_bech32_str(),
        }
    }

    pub fn identifier(&self) -> Identifier<Ed25519> {
        match self {
            Wallet::Account(account) => Identifier::from(account.identifier().as_ref().clone()),
            Wallet::UTxO(utxo) => utxo.identifier(),
            Wallet::Delegation(delegation) => delegation.identifier(),
        }
    }

    pub fn delegation_key(&self) -> Identifier<Ed25519> {
        match self {
            Wallet::Delegation(delegation) => {
                Identifier::from(delegation.last_delegation_identifier().as_ref().clone())
            }
            _ => unimplemented!(),
        }
    }

    pub fn add_input_with_value(&self, value: Value) -> Input {
        match self {
            Wallet::Account(account) => account.add_input_with_value(value),
            Wallet::UTxO(_utxo) => unimplemented!(),
            Wallet::Delegation(_delegation) => unimplemented!(),
        }
    }

    pub fn add_input<'a, Extra: Payload>(
        &self,
        payload: PayloadSlice<'a, Extra>,
        iobuilder: &mut InputOutputBuilder,
        fees: &LinearFee,
    ) -> Result<(), FragmentBuilderError>
    where
        LinearFee: FeeAlgorithm,
    {
        match self {
            Wallet::Account(account) => account.add_input(payload, iobuilder, fees),
            Wallet::UTxO(_utxo) => unimplemented!(),
            Wallet::Delegation(_delegation) => unimplemented!(),
        }
    }

    pub fn mk_witness(
        &self,
        block0_hash: &Hash,
        signing_data: &TransactionSignDataHash,
    ) -> Witness {
        match self {
            Wallet::Account(account) => account.mk_witness(block0_hash, signing_data),
            Wallet::UTxO(utxo) => utxo.mk_witness(block0_hash, signing_data),
            Wallet::Delegation(delegation) => delegation.mk_witness(block0_hash, signing_data),
        }
    }

    pub fn confirm_transaction(&mut self) {
        match self {
            Wallet::Account(account) => account.increment_counter(),
            _ => unimplemented!(),
        }
    }

    pub fn decrement_counter(&mut self) {
        match self {
            Wallet::Account(account) => account.decrement_counter(),
            _ => unimplemented!(),
        }
    }

    pub fn stake_key(&self) -> Option<UnspecifiedAccountIdentifier> {
        match &self {
            Wallet::Account(account) => Some(account.stake_key()),
            _ => unimplemented!(),
        }
    }

    pub fn delegation_cert_for_block0(&self, valid_until: BlockDate, pool_id: PoolId) -> Initial {
        FragmentBuilder::full_delegation_cert_for_block0(valid_until, self, pool_id)
    }

    pub fn transaction_to(
        &mut self,
        block0_hash: &Hash,
        fees: &LinearFee,
        valid_until: BlockDate,
        address: Address,
        value: Value,
    ) -> Result<Fragment, WalletError> {
        FragmentBuilder::new(block0_hash, fees, valid_until)
            .transaction(self, address, value)
            .map_err(WalletError::FragmentError)
    }

    pub fn transaction_to_many(
        &mut self,
        block0_hash: &Hash,
        fees: &LinearFee,
        valid_until: BlockDate,
        address: &[Address],
        value: Value,
    ) -> Result<Fragment, WalletError> {
        FragmentBuilder::new(block0_hash, fees, valid_until)
            .transaction_to_many(self, address, value)
            .map_err(WalletError::FragmentError)
    }

    pub fn issue_pool_retire_cert(
        &mut self,
        block0_hash: &Hash,
        fees: &LinearFee,
        valid_until: BlockDate,
        stake_pool: &StakePool,
    ) -> Result<Fragment, WalletError> {
        Ok(FragmentBuilder::new(block0_hash, fees, valid_until)
            .stake_pool_retire(vec![self], stake_pool))
    }

    pub fn issue_pool_registration_cert(
        &mut self,
        block0_hash: &Hash,
        fees: &LinearFee,
        valid_until: BlockDate,
        stake_pool: &StakePool,
    ) -> Result<Fragment, WalletError> {
        Ok(FragmentBuilder::new(block0_hash, fees, valid_until)
            .stake_pool_registration(self, stake_pool))
    }

    pub fn issue_pool_update_cert(
        &mut self,
        block0_hash: &Hash,
        fees: &LinearFee,
        valid_until: BlockDate,
        stake_pool: &StakePool,
        update_stake_pool: &StakePool,
    ) -> Result<Fragment, WalletError> {
        Ok(
            FragmentBuilder::new(block0_hash, fees, valid_until).stake_pool_update(
                vec![self],
                stake_pool,
                update_stake_pool,
            ),
        )
    }

    pub fn issue_full_delegation_cert(
        &mut self,
        block0_hash: &Hash,
        fees: &LinearFee,
        valid_until: BlockDate,
        stake_pool: &StakePool,
    ) -> Result<Fragment, WalletError> {
        Ok(FragmentBuilder::new(block0_hash, fees, valid_until).delegation(self, stake_pool))
    }

    pub fn issue_owner_delegation_cert(
        &mut self,
        block0_hash: &Hash,
        fees: &LinearFee,
        valid_until: BlockDate,
        stake_pool: &StakePool,
    ) -> Result<Fragment, WalletError> {
        Ok(FragmentBuilder::new(block0_hash, fees, valid_until).owner_delegation(self, stake_pool))
    }

    pub fn issue_split_delegation_cert(
        &mut self,
        block0_hash: &Hash,
        fees: &LinearFee,
        valid_until: BlockDate,
        distribution: Vec<(&StakePool, u8)>,
    ) -> Result<Fragment, WalletError> {
        Ok(FragmentBuilder::new(block0_hash, fees, valid_until)
            .delegation_to_many(self, distribution))
    }

    pub fn remove_delegation_cert(
        &mut self,
        block0_hash: &Hash,
        fees: &LinearFee,
        valid_until: BlockDate,
    ) -> Result<Fragment, WalletError> {
        Ok(FragmentBuilder::new(block0_hash, fees, valid_until).delegation_remove(self))
    }

    pub fn issue_vote_plan_cert(
        &mut self,
        block0_hash: &Hash,
        fees: &LinearFee,
        valid_until: BlockDate,
        vote_plan: &VotePlan,
    ) -> Result<Fragment, WalletError> {
        Ok(FragmentBuilder::new(block0_hash, fees, valid_until).vote_plan(self, vote_plan))
    }

    pub fn issue_vote_cast_cert(
        &mut self,
        block0_hash: &Hash,
        fees: &LinearFee,
        valid_until: BlockDate,
        vote_plan: &VotePlan,
        proposal_index: u8,
        choice: &Choice,
    ) -> Result<Fragment, WalletError> {
        match vote_plan.payload_type() {
            chain_impl_mockchain::vote::PayloadType::Public => Ok(FragmentBuilder::new(
                block0_hash,
                fees,
                valid_until,
            )
            .public_vote_cast(self, vote_plan, proposal_index, choice)),
            chain_impl_mockchain::vote::PayloadType::Private => Ok(FragmentBuilder::new(
                block0_hash,
                fees,
                valid_until,
            )
            .private_vote_cast(self, vote_plan, proposal_index, choice)),
        }
    }

    pub fn issue_encrypted_tally_cert(
        &mut self,
        block0_hash: &Hash,
        fees: &LinearFee,
        valid_until: BlockDate,
        vote_plan: &VotePlan,
    ) -> Result<Fragment, WalletError> {
        Ok(FragmentBuilder::new(block0_hash, fees, valid_until).encrypted_tally(self, vote_plan))
    }

    pub fn issue_vote_tally_cert(
        &mut self,
        block0_hash: &Hash,
        fees: &LinearFee,
        valid_until: BlockDate,
        vote_plan: &VotePlan,
        tally_type: VoteTallyPayload,
    ) -> Result<Fragment, WalletError> {
        Ok(FragmentBuilder::new(block0_hash, fees, valid_until)
            .vote_tally(self, vote_plan, tally_type))
    }

    pub fn to_committee_id(&self) -> CommitteeIdDef {
        CommitteeIdDef::from(CommitteeId::from(
            self.address().1.public_key().unwrap().clone(),
        ))
    }

    pub fn update_counter(&mut self, counter: u32) {
        if let Wallet::Account(account) = self {
            account.set_counter(counter)
        }
    }
}

impl From<Wallet> for WalletLib {
    fn from(wallet: Wallet) -> WalletLib {
        let address_data = match wallet {
            Wallet::Account(account) => AddressData::new(
                account.signing_key().as_ref().clone(),
                Some(account.internal_counter()),
                account.address().into(),
            ),
            Wallet::UTxO(utxo) => AddressData::new(
                EitherEd25519SecretKey::Extended(utxo.last_signing_key().as_ref().clone()),
                None,
                utxo.address().into(),
            ),
            Wallet::Delegation(delegation) => AddressData::new(
                EitherEd25519SecretKey::Extended(delegation.last_signing_key().as_ref().clone()),
                None,
                delegation.address().into(),
            ),
        };
        let address_data_value = AddressDataValue::new(address_data, ValueLib(0));
        WalletLib::from_address_data_value(address_data_value)
    }
}
