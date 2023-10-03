use std::collections::{HashMap, HashSet};

use mempool::MempoolReadHandleFactory;
use primitives::Address;
use rayon::ThreadPoolBuilder;
use storage::vrrbdb::{StateStoreReadHandleFactory, ClaimStoreReadHandleFactory};
use vrrb_core::transactions::TransactionKind;
use vrrb_core::{account::Account, claim::Claim};

use crate::{
    claim_validator::ClaimValidator,
    result::{Result, ValidatorError},
    txn_validator::TxnValidator,
    validator_core::{Core, CoreId},
};

#[derive(Debug)]
pub struct ValidatorCoreManager {
    core_pool: rayon::ThreadPool,
    mempool_reader: MempoolReadHandleFactory,
    state_reader: StateStoreReadHandleFactory,
    claim_reader: ClaimStoreReadHandleFactory,
}

impl Clone for ValidatorCoreManager {
    fn clone(&self) -> Self {
        let cores = self.core_pool.current_num_threads();

        // NOTE: rm this unwrap somehow
        let core_pool = ThreadPoolBuilder::new().num_threads(cores).build().unwrap();
        let mempool_reader = self.mempool_reader.clone();
        let state_reader = self.state_reader.clone();
        let claim_reader = self.claim_reader.clone();

        Self { core_pool, mempool_reader, state_reader, claim_reader }
    }
}

impl ValidatorCoreManager {
    pub fn new(
        cores: usize, 
        mempool_reader: MempoolReadHandleFactory, 
        state_reader: StateStoreReadHandleFactory,
        claim_reader: ClaimStoreReadHandleFactory,
    ) -> Result<Self> {
        let core_pool = ThreadPoolBuilder::new()
            .num_threads(cores)
            .build()
            .map_err(|err| {
                ValidatorError::Other(format!("Failed to create validator core pool: {err}"))
            })?;

        Ok(Self { core_pool, mempool_reader, state_reader, claim_reader })
    }

    pub fn validate(
        &mut self,
        account_state: &HashMap<Address, Account>,
        batch: Vec<TransactionKind>,
    ) -> HashSet<(TransactionKind, crate::txn_validator::Result<()>)> {
        // ) -> HashSet<(Txn, bool)> {
        self.core_pool.install(|| {
            let valcore = Core::new(
                self.core_pool.current_thread_index().unwrap_or(0) as CoreId,
                TxnValidator::new(),
                ClaimValidator,
            );
            valcore.process_transactions(account_state, batch)
        })
    }

    pub fn validate_claims(
        &mut self,
        claims: Vec<Claim>,
    ) -> HashSet<(Claim, crate::claim_validator::Result<()>)> {
        self.core_pool.install(|| {
            let valcore = Core::new(
                self.core_pool.current_thread_index().unwrap_or(0) as CoreId,
                TxnValidator::new(),
                ClaimValidator,
            );
            valcore.process_claims(claims)
        })
    }
}
