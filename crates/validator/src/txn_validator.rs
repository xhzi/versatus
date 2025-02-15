use std::{collections::HashMap, result::Result as StdResult, str::FromStr};

use primitives::Address;
use vrrb_core::{account::Account, keypair::KeyPair};
use vrrb_core::transactions::{Transaction, TransactionKind};

pub type Result<T> = StdResult<T, TxnValidatorError>;

pub const ADDRESS_PREFIX: &str = "0x192";

pub enum TxnFees {
    Slow,
    Fast,
    Instant,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq, Hash)]
pub enum TxnValidatorError {
    #[error("invalid sender")]
    InvalidSender,

    #[error("missing sender address")]
    SenderAddressMissing,

    #[error("invalid sender address")]
    SenderAddressIncorrect,

    #[error("invalid sender public key")]
    SenderPublicKeyIncorrect,

    #[error("missing receiver address")]
    ReceiverAddressMissing,

    #[error("invalid receiver address")]
    ReceiverAddressIncorrect,

    #[error("timestamp {0} is outside of the permitted date range [0, {1}]")]
    OutOfBoundsTimestamp(i64, i64),

    #[error("value {0} is outside of the permitted range [{1}, {2}]")]
    OutOfBounds(String, String, String),

    #[error("invalid amount")]
    TxnAmountIncorrect,

    #[error("invalid signature")]
    TxnSignatureIncorrect,

    #[error("invalid threshold signature")]
    TxnSignatureTresholdIncorrect,

    #[error("value not found")]
    NotFound,

    #[error("account not found within state state_snapshot: {0}")]
    AccountNotFound(String),
}

#[derive(Debug, Clone, Default)]
// TODO: make validator configurable
pub struct TxnValidator;

impl TxnValidator {
    /// Creates a new Txn validator
    pub fn new() -> TxnValidator {
        TxnValidator
    }

    /// An entire Txn validator
    // TODO: include fees and signature threshold.
    pub fn validate(&self, account_state: &HashMap<Address, Account>, txn: &TransactionKind) -> Result<()> {
        self.validate_structure(account_state, txn)
    }

    /// An entire Txn structure validator
    pub fn validate_structure(
        &self,
        account_state: &HashMap<Address, Account>,
        txn: &TransactionKind,
    ) -> Result<()> {
        self.validate_amount(account_state, txn)
            .and_then(|_| self.validate_public_key(txn))
            .and_then(|_| self.validate_sender_address(txn))
            .and_then(|_| self.validate_receiver_address(txn))
            .and_then(|_| self.validate_signature(txn))
            .and_then(|_| self.validate_timestamp(txn))
    }

    /// Txn signature validator.
    pub fn validate_signature(&self, txn: &TransactionKind) -> Result<()> {
        let txn_signature = txn.signature();
        if !txn_signature.to_string().is_empty() {
            KeyPair::verify_ecdsa_sign(
                // TODO: revisit this verification
                format!("{:?}", txn.signature()),
                txn.build_payload().as_bytes(),
                txn.sender_public_key().to_string().as_bytes().to_vec(),
            )
            .map_err(|_| TxnValidatorError::TxnSignatureIncorrect)
        } else {
            Err(TxnValidatorError::TxnSignatureIncorrect)
        }
    }

    /// Txn public key validator
    pub fn validate_public_key(&self, txn: &TransactionKind) -> Result<()> {
        if !txn.sender_public_key().to_string().is_empty() {
            Ok(())
        } else {
            Err(TxnValidatorError::SenderPublicKeyIncorrect)
        }
    }

    /// Txn sender validator
    // TODO, to be synchronized with Wallet.
    pub fn validate_sender_address(&self, txn: &TransactionKind) -> Result<()> {
        if !txn.sender_address().to_string().is_empty()
            && txn.sender_address().to_string().starts_with(ADDRESS_PREFIX)
            && txn.sender_address().to_string().len() > 10
        {
            Ok(())
        } else {
            Err(TxnValidatorError::SenderAddressMissing)
        }
    }

    /// Txn receiver validator
    // TODO, to be synchronized with Wallet.
    pub fn validate_receiver_address(&self, txn: &TransactionKind) -> Result<()> {
        if !txn.receiver_address().to_string().is_empty()
            && txn.receiver_address().to_string().starts_with(ADDRESS_PREFIX)
            && txn.receiver_address().to_string().len() > 10
        {
            Ok(())
        } else {
            Err(TxnValidatorError::ReceiverAddressMissing)
        }
    }

    /// Txn timestamp validator
    pub fn validate_timestamp(&self, txn: &TransactionKind) -> Result<()> {
        let timestamp = chrono::offset::Utc::now().timestamp();

        // TODO: revisit seconds vs nanoseconds for timestamp
        // let timestamp = duration.as_nanos();
        if txn.timestamp() > 0 && txn.timestamp() < timestamp {
            Ok(())
        } else {
            Err(TxnValidatorError::OutOfBoundsTimestamp(
                txn.timestamp(),
                timestamp,
            ))
        }
    }

    /// Txn receiver validator
    // TODO, to be synchronized with transaction fees.
    pub fn validate_amount(
        &self,
        account_state: &HashMap<Address, Account>,
        txn: &TransactionKind,
    ) -> Result<()> {
        let address = txn.sender_address();
        if let Ok(address) = secp256k1::PublicKey::from_str(address.to_string().as_str()) {
            let account = account_state.get(&Address::new(address)).unwrap();
            if (account.credits() - account.debits())
                .checked_sub(txn.amount())
                .is_none()
            {
                return Err(TxnValidatorError::TxnAmountIncorrect);
            };
        } else {
            return Err(TxnValidatorError::SenderAddressIncorrect);
        }

        Ok(())
    }
}
