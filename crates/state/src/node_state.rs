use std::collections::HashMap;
use std::ffi::OsStr;
use std::os;
use std::path::PathBuf;
use std::{fs, sync::Arc};

use accountable::accountable::Accountable;
/// This module contains the Network State struct (which will be replaced with
/// the Left-Right State Trie)
use lr_trie::{LeftRightTrie, H256};
use lrdb::Account;
use patriecia::db::MemoryDB;
use primitives::PublicKey;
use ritelinked::LinkedHashMap;
// use state_trie::StateTrie;
use telemetry::{error, info};
// use reward::reward::{Reward, RewardState};
use serde::{Deserialize, Serialize};
use sha256::digest_bytes;

use crate::result::Result;
use crate::types::{
    CreditsHash, CreditsRoot, DebitsHash, DebitsRoot, LedgerBytes, StateHash, StatePath,
    StateRewardState, StateRoot,
};
use crate::StateError;

// let ftjson = OsStr::new("json");

/// The Node State struct, contains basic information required to determine
/// the current state of the network.
//TODO: Replace `ledger`, `credits`, `debits`, with LR State Trie
//TODO: Replace `state_hash` with LR State Trie Root.
// #[derive(Debug, Serialize, Deserialize)]
#[derive(Debug)]
pub struct NodeState {
    /// Path to database
    pub path: StatePath,
    // /// Reward state of the network
    // pub reward_state: StateRewardState,
    // // the last state hash -> sha256 hash of credits, debits & reward state.
    // pub state_hash: StateRoot,
    // _mempool:
    state_trie: LeftRightTrie<MemoryDB>,
    tx_trie: LeftRightTrie<MemoryDB>,
}

impl Clone for NodeState {
    /// Warning: do not use yet as lr_trie doesn't fully implement clone yet.
    fn clone(&self) -> NodeState {
        NodeState {
            path: self.path.clone(),
            state_trie: self.state_trie.clone(),
            tx_trie: self.tx_trie.clone(),
        }
    }
}

impl From<NodeStateValues> for NodeState {
    fn from(node_state_values: NodeStateValues) -> Self {
        let mut state_trie = LeftRightTrie::new(Arc::new(MemoryDB::new(true)));

        // let mut tx_trie = LeftRightTrie::new(Arc::new(MemoryDB::new(true)));
        // let mut state_trie = StateTrie::new(Arc::new(MemoryDB::new(true)));

        let mapped_state = node_state_values
            .state
            .into_iter()
            .map(|(key, acc)| (key, acc))
            // .collect::<Vec<(Vec<u8>, Vec<u8>)>>();
            .collect();

        state_trie.extend(mapped_state);

        Self {
            path: PathBuf::new(),
            state_trie,
            tx_trie: LeftRightTrie::new(Arc::new(MemoryDB::new(true))),
            // state_trie: LeftRightTrie::new(Arc::new(MemoryDB::new(true))),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct NodeStateValues {
    pub txns: LinkedHashMap<PublicKey, Account>,
    pub state: LinkedHashMap<PublicKey, Account>,
}

impl From<&NodeState> for NodeStateValues {
    fn from(node_state: &NodeState) -> Self {
        Self {
            txns: LinkedHashMap::new(),
            state: LinkedHashMap::new(),
        }
    }
}

impl NodeStateValues {
    /// Converts a vector of bytes into a Network State or returns an error if
    /// it's unable to
    pub fn from_bytes(data: &[u8]) -> Result<NodeStateValues> {
        serde_json::from_slice::<NodeStateValues>(data)
            .map_err(|err| StateError::Other(err.to_string()))
    }
}

impl NodeState {
    pub fn new(path: std::path::PathBuf) -> Self {
        // TODO: replace memorydb with real backing db later
        let mem_db = MemoryDB::new(true);
        let backing_db = Arc::new(mem_db);
        let state_trie = LeftRightTrie::new(backing_db.clone());
        let tx_trie = LeftRightTrie::new(backing_db);

        Self {
            path,
            state_trie,
            tx_trie,
        }
    }

    /// Dumps a hex string representation of `NodeStateValues` to file.
    pub fn dump_to_file(&self) -> Result<()> {
        //TODO: discuss if hex::encode is worth implementing
        todo!()
    }

    /// Generates a backup of NodeState serialized into JSON at the specified path.
    pub fn serialize_to_json(&self) -> Result<()> {
        let node_state_values = NodeStateValues::from(self);

        let serialized = serde_json::to_vec(&node_state_values)
            .map_err(|err| StateError::Other(err.to_string()))?;

        fs::write(&self.path, serialized).map_err(|err| StateError::Other(err.to_string()))?;

        Ok(())
    }

    /// Restores the network state from a serialized file stored on disk.
    pub fn restore(path: &PathBuf) -> Result<NodeState> {
        //NOTE: refactor this naive impl later
        let ext = path
            .extension()
            .ok_or_else(|| {
                StateError::Other(format!("file extension not found on file {:?}", path))
            })?
            .to_str()
            .ok_or_else(|| {
                StateError::Other("file extension is not a valid UTF-8 string".to_string())
            })?;

        match ext {
            // TODO: add more match arms to support more backup filetypes
            "json" => NodeState::restore_from_json_file(path),
            _ => Err(StateError::Other(format!(
                "file extension not found on file {:?}",
                &path
            ))),
        }
    }

    fn restore_from_json_file(path: &PathBuf) -> Result<NodeState> {
        let read = fs::read(path).map_err(|err| StateError::Other(err.to_string()))?;

        let deserialized: NodeStateValues =
            serde_json::from_slice(&read).map_err(|err| StateError::Other(err.to_string()))?;

        let mut node_state = NodeState::from(deserialized);
        node_state.path = path.to_owned();

        Ok(node_state)
    }

    /// Returns the current state trie's root hash.
    pub fn root_hash(&self) -> Option<H256> {
        self.state_trie.root()
    }

    // MOCK TEST  FUNCTION
    pub fn values(&self) {
        let mut accounts = HashMap::new();
        let iter = self.state_trie.handle().iter();
        for (k, account_bytes) in iter {
            let account: Account = serde_json::from_slice(&account_bytes).unwrap();
            accounts.insert(k, account);
        }

        dbg!(&accounts);
    }

    pub fn add_account(&self) {
        todo!()
    }

    pub fn get_account(&self) {
        todo!()
    }
}

// if let Err(err) = fs::write(path, serialized) {
//     error!("Unable to write state to file. Reason: {0}", err);
// }

// if let Err(_) = fs::write(self.path.clone(), hex::encode(self.as_bytes())) {
//     info!("Error dumping ledger to file");
// };

/*
impl<'de> NetworkState {

    /// Dumps a new ledger (serialized in a vector of bytes) to a file.
    pub fn set_ledger(&mut self, ledger_bytes: LedgerBytes) {
        self.ledger = ledger_bytes;
        self.dump_to_file();
    }

    /// Sets a new `RewardState` to the `reward_state` filed in the
    /// `NetworkState` and dumps the resulting new state to the file
    pub fn set_reward_state(&mut self, reward_state: RewardState) {
        self.reward_state = Some(reward_state);
        self.dump_to_file();
    }

    /// Gets the balance of a given address from the network state
    pub fn get_balance(&self, address: &str) -> u128 {
        let credits = self.get_account_credits(address);
        let debits = self.get_account_debits(address);

        if let Some(balance) = credits.checked_sub(debits) {
            return balance;
        } else {
            return 0u128;
        }
    }

    /// Calculates a new/updated `CreditsHash`
    pub fn credit_hash<A: Accountable, R: Accountable>(
        self,
        txns: &LinkedHashMap<String, A>,
        reward: R,
    ) -> CreditsHash {
        let mut credits = LinkedHashMap::new();

        txns.iter().for_each(|(_txn_id, txn)| {
            if let Some(entry) = credits.get_mut(&txn.receivable()) {
                *entry += txn.get_amount()
            } else {
                credits.insert(txn.receivable(), txn.get_amount());
            }
        });

        if let Some(entry) = credits.get_mut(&reward.receivable()) {
            *entry += reward.get_amount()
        } else {
            credits.insert(reward.receivable(), reward.get_amount());
        }

        if let Some(chs) = self.credits {
            return digest_bytes(format!("{},{:?}", chs, credits).as_bytes());
        } else {
            return digest_bytes(format!("{:?},{:?}", self.credits, credits).as_bytes());
        }
    }

    /// Calculates a new/updated `DebitsHash`
    pub fn debit_hash<A: Accountable>(self, txns: &LinkedHashMap<String, A>) -> DebitsHash {
        let mut debits = LinkedHashMap::new();
        txns.iter().for_each(|(_txn_id, txn)| {
            if let Some(payable) = txn.payable() {
                if let Some(entry) = debits.get_mut(&payable) {
                    *entry += txn.get_amount()
                } else {
                    debits.insert(payable.clone(), txn.get_amount());
                }
            }
        });

        if let Some(dhs) = self.debits {
            return digest_bytes(format!("{},{:?}", dhs, debits).as_bytes());
        } else {
            return digest_bytes(format!("{:?},{:?}", self.debits, debits).as_bytes());
        }
    }

    /// Hashes the current credits, debits and reward state and returns a new
    /// `StateHash`
    pub fn hash<A: Accountable, R: Accountable>(
        &mut self,
        txns: &LinkedHashMap<String, A>,
        reward: R,
    ) -> StateHash {
        let credit_hash = self.clone().credit_hash(&txns, reward);
        let debit_hash = self.clone().debit_hash(&txns);
        let reward_state_hash = digest_bytes(format!("{:?}", self.reward_state).as_bytes());
        let payload = format!(
            "{:?},{:?},{:?},{:?}",
            self.state_hash, credit_hash, debit_hash, reward_state_hash
        );
        let new_state_hash = digest_bytes(payload.as_bytes());
        new_state_hash
    }

    /// Updates the ledger and dumps it to a file
    pub fn dump<A: Accountable>(
        &mut self,
        txns: &LinkedHashMap<String, A>,
        reward: Reward,
        claims: &LinkedHashMap<String, Claim>,
        miner_claim: Claim,
        hash: &String,
    ) {
        let mut ledger = Ledger::<Claim>::from_bytes(self.ledger.clone());

        txns.iter().for_each(|(_txn_id, txn)| {
            if let Some(entry) = ledger.credits.get_mut(&txn.receivable()) {
                *entry += txn.get_amount();
            } else {
                ledger.credits.insert(txn.receivable(), txn.get_amount());
            }

            if let Some(payable) = txn.payable() {
                if let Some(entry) = ledger.debits.get_mut(&payable) {
                    *entry += txn.get_amount();
                } else {
                    ledger.debits.insert(payable, txn.get_amount());
                }
            }
        });

        claims.iter().for_each(|(k, v)| {
            ledger.claims.insert(k.clone(), v.clone());
        });

        ledger.claims.insert(miner_claim.get_pubkey(), miner_claim);

        if let Some(entry) = ledger.credits.get_mut(&reward.receivable()) {
            *entry += reward.get_amount();
        } else {
            ledger
                .credits
                .insert(reward.receivable(), reward.get_amount());
        }

        self.update_reward_state(reward.clone());
        self.update_state_hash(hash);
        self.update_credits_and_debits(&txns, reward.clone());

        let ledger_hex = hex::encode(ledger.clone().as_bytes());
        if let Err(_) = fs::write(self.path.clone(), ledger_hex) {
            info!("Error writing ledger hex to file");
        };

        self.ledger = ledger.as_bytes();
    }

    // TODO: refactor to handle NetworkState nonce_up() a different way, since
    // closure requires explicit types and explicit type specification would
    // lead to cyclical dependencies.
    /// nonces all claims in the ledger up one.
    pub fn nonce_up(&mut self) {
        let mut new_claim_map: LinkedHashMap<String, Claim> = LinkedHashMap::new();
        let claims: LinkedHashMap<String, Claim> = self.get_claims().clone();
        claims.iter().for_each(|(pk, claim)| {
            let mut new_claim = claim.clone();
            new_claim.nonce_up();
            new_claim_map.insert(pk.clone(), new_claim.clone());
        });

        let mut ledger = Ledger::from_bytes(self.ledger.clone());
        ledger.claims = new_claim_map;
        self.ledger = ledger.as_bytes();
    }

    /// Abandons a claim in the Ledger
    pub fn abandoned_claim(&mut self, hash: String) {
        let mut ledger: Ledger<Claim> = Ledger::from_bytes(self.ledger.clone());
        ledger.claims.retain(|_, v| v.hash != hash);
        self.ledger = ledger.as_bytes();
        self.dump_to_file();
    }

    /// Restors the ledger from a hex string representation stored in a file to
    /// a proper ledger
    pub fn restore_ledger(&self) -> Ledger<Claim> {
        let network_state_hex = fs::read_to_string(self.path.clone()).unwrap();
        let bytes = hex::decode(network_state_hex);
        if let Ok(state_bytes) = bytes {
            if let Ok(network_state) = NetworkState::from_bytes(state_bytes) {
                return Ledger::from_bytes(network_state.ledger.clone());
            } else {
                return Ledger::new();
            }
        } else {
            return Ledger::new();
        }
    }

    /// Updates the credit and debit hashes in the network state.
    pub fn update_credits_and_debits<A: Accountable>(
        &mut self,
        txns: &LinkedHashMap<String, A>,
        reward: Reward,
    ) {
        let chs = self.clone().credit_hash(txns, reward);
        let dhs = self.clone().debit_hash(txns);
        self.credits = Some(chs);
        self.debits = Some(dhs);
    }

    /// Updates the reward state given a new reward of a specific category
    pub fn update_reward_state(&mut self, reward: Reward) {
        if let Some(category) = reward.get_category() {
            if let Some(mut reward_state) = self.reward_state.clone() {
                reward_state.update(category);
                self.reward_state = Some(reward_state);
            }
        }
    }

    /// Updates the state hash
    pub fn update_state_hash(&mut self, hash: &StateHash) {
        self.state_hash = Some(hash.clone());
    }

    /// Returns the credits from the ledger
    pub fn get_credits(&self) -> LinkedHashMap<String, u128> {
        Ledger::<Claim>::from_bytes(self.ledger.clone())
            .credits
            .clone()
    }

    /// Returns the debits from the ledger
    pub fn get_debits(&self) -> LinkedHashMap<String, u128> {
        Ledger::<Claim>::from_bytes(self.ledger.clone())
            .debits
            .clone()
    }

    /// Returns the claims from the ledger
    pub fn get_claims(&self) -> LinkedHashMap<String, Claim> {
        Ledger::<Claim>::from_bytes(self.ledger.clone())
            .claims
            .clone()
    }

    /// Returns the `RewardState` from the `NewtorkState`
    pub fn get_reward_state(&self) -> Option<RewardState> {
        self.reward_state.clone()
    }

    /// Gets the credits from a specific account
    pub fn get_account_credits(&self, address: &str) -> u128 {
        let credits = self.get_credits();
        if let Some(amount) = credits.get(address) {
            return *amount;
        } else {
            return 0u128;
        }
    }

    /// Gets the debits from a specific account
    pub fn get_account_debits(&self, address: &str) -> u128 {
        let debits = self.get_debits();
        if let Some(amount) = debits.get(address) {
            return *amount;
        } else {
            return 0u128;
        }
    }

    /// Replaces the current ledger with a new ledger
    pub fn update_ledger(&mut self, ledger: Ledger<Claim>) {
        self.ledger = ledger.as_bytes();
    }

    /// Calculates the lowest pointer sums given the claim map
    pub fn get_lowest_pointer(&self, block_seed: u128) -> Option<(String, u128)> {
        let claim_map = self.get_claims();
        let mut pointers = claim_map
            .iter()
            .map(|(_, claim)| return (claim.clone().hash, claim.clone().get_pointer(block_seed)))
            .collect::<Vec<_>>();

        pointers.retain(|(_, v)| !v.is_none());

        let mut base_pointers = pointers
            .iter()
            .map(|(k, v)| {
                return (k.clone(), v.unwrap());
            })
            .collect::<Vec<_>>();

        if let Some(min) = base_pointers.clone().iter().min_by_key(|(_, v)| v) {
            base_pointers.retain(|(_, v)| *v == min.1);
            Some(base_pointers[0].clone())
        } else {
            None
        }
    }

    /// Slashes a claim of a miner that proposes an invalid block or spams the
    /// network
    pub fn slash_claims(&mut self, bad_validators: Vec<String>) {
        let mut ledger: Ledger<Claim> = Ledger::from_bytes(self.ledger.clone());
        bad_validators.iter().for_each(|k| {
            if let Some(claim) = ledger.claims.get_mut(&k.to_string()) {
                claim.eligible = false;
            }
        });
        self.ledger = ledger.as_bytes();
        self.dump_to_file()
    }


    /// Returns a serialized representation of the credits map as a vector of
    /// bytes
    pub fn credits_as_bytes(credits: &LinkedHashMap<String, u128>) -> Vec<u8> {
        NetworkState::credits_to_string(credits).as_bytes().to_vec()
    }

    /// Returns a serialized representation of the credits map as a string
    pub fn credits_to_string(credits: &LinkedHashMap<String, u128>) -> String {
        serde_json::to_string(credits).unwrap()
    }

    /// Returns a credits map from a byte array
    pub fn credits_from_bytes(data: &[u8]) -> LinkedHashMap<String, u128> {
        serde_json::from_slice::<LinkedHashMap<String, u128>>(data).unwrap()
    }

    /// Returns a vector of bytes representing the debits map
    pub fn debits_as_bytes(debits: &LinkedHashMap<String, u128>) -> Vec<u8> {
        NetworkState::debits_to_string(debits).as_bytes().to_vec()
    }

    /// Returns a string representing the debits map
    pub fn debits_to_string(debits: &LinkedHashMap<String, u128>) -> String {
        serde_json::to_string(debits).unwrap()
    }

    /// Converts a byte array representing the debits map back into the debits
    /// map
    pub fn debits_from_bytes(data: &[u8]) -> LinkedHashMap<String, u128> {
        serde_json::from_slice::<LinkedHashMap<String, u128>>(data).unwrap()
    }

    /// Returns a vector of bytes representing the claim map
    pub fn claims_as_bytes<C: Ownable + Serialize>(claims: &LinkedHashMap<u128, C>) -> Vec<u8> {
        NetworkState::claims_to_string(claims).as_bytes().to_vec()
    }

    /// Returns a string representation of the claim map
    pub fn claims_to_string<C: Ownable + Serialize>(claims: &LinkedHashMap<u128, C>) -> String {
        serde_json::to_string(claims).unwrap()
    }

    /// Returns a claim map from an array of bytes
    pub fn claims_from_bytes<C: Ownable + Deserialize<'de>>(
        data: &'de [u8],
    ) -> LinkedHashMap<u128, C> {
        serde_json::from_slice::<LinkedHashMap<u128, C>>(data).unwrap()
    }

    /// Returns a block (representing the last block) from a byte array
    pub fn last_block_from_bytes<D: Deserialize<'de>>(data: &'de [u8]) -> D {
        serde_json::from_slice::<D>(data).unwrap()
    }

    /// Serializes the network state as a vector of bytes
    pub fn as_bytes(&self) -> Vec<u8> {
        self.to_string().as_bytes().to_vec()
    }

    /// Converts a vector of bytes into a Network State or returns an error if
    /// it's unable to
    pub fn from_bytes(data: Vec<u8>) -> Result<NetworkState, serde_json::error::Error> {
        serde_json::from_slice::<NetworkState>(&data.clone())
    }

    /// Serializes the network state into a string
    pub fn to_string(&self) -> String {
        serde_json::to_string(self).unwrap()
    }

    /// Deserializes the network state from a string
    pub fn from_string(string: String) -> NetworkState {
        serde_json::from_str::<NetworkState>(&string).unwrap()
    }

    /// creates a Ledger from the network state
    pub fn db_to_ledger(&self) -> Ledger<Claim> {
        let credits = self.get_credits();
        let debits = self.get_debits();
        let claims = self.get_claims();

        Ledger {
            credits,
            debits,
            claims,
        }
    }
}
*/
