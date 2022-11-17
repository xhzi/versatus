// This file contains code for creating blocks to be proposed, including the
// genesis block and blocks being mined.
#![allow(unused_imports)]
#![allow(dead_code)]
use std::fmt;

use accountable::accountable::Accountable;
use claim::claim::Claim;
use log::info;
use lr_trie::LeftRightTrie;
use patriecia::db::Database;
use primitives::types::{Epoch, RawSignature, GENESIS_EPOCH};
use rand::Rng;
use reward::reward::{Category, RewardState, GENESIS_REWARD};
use ritelinked::LinkedHashMap;
use serde::{Deserialize, Serialize};
use sha256::digest;
use state::state::NetworkState;
#[allow(deprecated)]
use txn::txn::{Transaction, Txn};
use verifiable::verifiable::Verifiable;

use crate::{
    genesis,
    header::BlockHeader,
    invalid::{InvalidBlockError, InvalidBlockErrorReason},
};

pub const NANO: u128 = 1;
pub const MICRO: u128 = NANO * 1000;
pub const MILLI: u128 = MICRO * 1000;
pub const SECOND: u128 = MILLI * 1000;
const VALIDATOR_THRESHOLD: f64 = 0.60;
pub type CurrentUtility = u128;


#[derive(Clone, Debug, Serialize, Deserialize)]
#[repr(C)]
pub struct Block {
    pub header: BlockHeader,
    pub neighbors: Option<Vec<BlockHeader>>,
    pub height: u128,
    // TODO: replace with Tx Trie Root
    pub txns: LinkedHashMap<String, Txn>,
    // TODO: Replace with Claim Trie Root
    pub claims: LinkedHashMap<String, Claim>,
    pub hash: String,
    pub received_at: Option<u128>,
    pub received_from: Option<String>,
    // TODO: Replace with map of all abandoned claims in the even more than 1 miner is faulty when
    // they are entitled to mine
    pub abandoned_claim: Option<Claim>,

    /// Quorum signature needed for finalizing the block and locking the chain
    pub threshold_signature: Option<RawSignature>,

    /// Epoch for which block was created
    pub epoch: Epoch,

    /// Measurement of utility for the chain
    pub utility: u128,
}

impl Block {
    // Returns a result with either a tuple containing the genesis block and the
    // updated account state (if successful) or an error (if unsuccessful)
    pub fn genesis(reward_state: &RewardState, claim: Claim, secret_key: String) -> Option<Block> {
        // Create the genesis header
        let header = BlockHeader::genesis(0, reward_state, claim.clone(), secret_key);
        // Create the genesis state hash
        // TODO: Replace with state trie root
        let state_hash = digest(
            format!(
                "{},{}",
                header.last_hash,
                digest("Genesis_State_Hash".as_bytes())
            )
            .as_bytes(),
        );

        // Replace with claim trie
        let mut claims = LinkedHashMap::new();
        claims.insert(claim.clone().pubkey, claim);

        #[cfg(mainnet)]
        let txns = genesis::generate_genesis_txns();

        // TODO: Genesis block on local/testnet should generate either a faucet for
        // tokens, or fill some initial accounts so that testing can be executed

        #[cfg(not(mainnet))]
        let txns = LinkedHashMap::new();

        let genesis = Block {
            header,
            neighbors: None,
            height: 0,
            txns,
            claims,
            hash: state_hash,
            received_at: None,
            received_from: None,
            abandoned_claim: None,
            threshold_signature: None,
            utility: 0,
            epoch: GENESIS_EPOCH,
        };

        // Update the State Trie & Tx Trie with the miner and new block, this will also
        // set the values to the network state. Unwrap the result and assign it
        // to the variable updated_account_state to be returned by this method.

        Some(genesis)
    }

    /// The mine method is used to generate a new block (and an updated account
    /// state with the reward set to the miner wallet's balance), this will
    /// also update the network state with a new confirmed state.
    #[allow(clippy::too_many_arguments)]
    #[allow(deprecated)]
    pub fn mine<D: Database>(
        claim: Claim,      // The claim entitling the miner to mine the block.
        last_block: Block, // The last block, which contains the current block reward.
        txns: LinkedHashMap<String, Txn>,
        claims: LinkedHashMap<String, Claim>,
        claim_map_hash: Option<String>,
        reward_state: &RewardState,
        network_state: &LeftRightTrie<D>,
        neighbors: Option<Vec<BlockHeader>>,
        abandoned_claim: Option<Claim>,
        signature: String,
        epoch: Epoch,
    ) -> (Option<Block>, CurrentUtility) {
        // TODO: Replace with Tx Trie Root
        let txn_hash = {
            let mut txn_vec = vec![];
            txns.iter().for_each(|(_, v)| {
                let bytes = v.as_bytes();
                txn_vec.extend(bytes);
            });
            digest(&*txn_vec)
        };

        // TODO: Remove there should be no neighbors
        let neighbors_hash = {
            let mut neighbors_vec = vec![];
            if let Some(neighbors) = &neighbors {
                neighbors.iter().for_each(|v| {
                    neighbors_vec.extend(v.as_bytes());
                });
                Some(digest(&*neighbors_vec))
            } else {
                None
            }
        };

        // TODO: Fix after replacing neighbors and tx hash/claim hash with respective
        // Trie Roots
        let header = BlockHeader::new(
            last_block.clone(),
            reward_state,
            claim,
            txn_hash,
            claim_map_hash,
            neighbors_hash,
            signature,
        );

        // TODO: Discuss whether local clock works well enough for this purpose of
        // guaranteeing at least 1 second between blocks or whether some other
        // mechanism may serve the purpose better, or whether simply sequencing proposed
        // blocks and allowing validator network to determine how much time
        // between blocks has passed.
        if let Some(time) = header.timestamp.checked_sub(last_block.header.timestamp) {
            if (time / SECOND) < 1 {
                return (None, 0u128);
            }
        } else {
            return (None, 0u128);
        }

        let height = last_block.height + 1;

        let utility_amount: u128 = txns.iter().map(|(_, txn)| txn.get_amount()).sum();
        let mut block = Block {
            header: header.clone(),
            neighbors,
            height,
            txns,
            claims,
            hash: header.last_hash,
            received_at: None,
            received_from: None,
            abandoned_claim,
            threshold_signature: None,
            utility: 0,
            epoch,
        };
        let mut adjustment_next_epoch = 0;
        if block.epoch != last_block.epoch {
            block.utility = utility_amount;
            adjustment_next_epoch = if block.utility > last_block.utility {
                (block.utility as f64 * 0.01) as u128
            } else {
                (block.utility as f64 * -0.01) as u128
            };
        } else {
            block.utility = utility_amount + last_block.utility;
        }


        // block.header.block_reward.clone());
        //TODO: Apply changes from txns to the state
        let hash = network_state.root().unwrap();
        block.hash = hash.to_string();
        (Some(block), adjustment_next_epoch)
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        self.to_string().as_bytes().to_vec()
    }

    pub fn from_bytes(data: &[u8]) -> Block {
        let mut buffer: Vec<u8> = vec![];

        data.iter().for_each(|x| buffer.push(*x));

        let to_string = String::from_utf8(buffer).unwrap();

        serde_json::from_str::<Block>(&to_string).unwrap()
    }

    // TODO: Consider renaming to `serialize_to_string`
    #[allow(clippy::inherent_to_string_shadow_display)]
    pub fn to_string(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
}

impl fmt::Display for Block {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Block(\n \
            header: {:?},\n",
            self.header
        )
    }
}

// TODO: Rewrite Verifiable to comport with Masternode Quorum Validation
// Protocol
impl Verifiable for Block {
    type Dependencies = (NetworkState, RewardState);
    type Error = InvalidBlockError;
    type Item = Block;

    fn verifiable(&self) -> bool {
        true
    }

    #[allow(unused_variables)]
    fn valid(
        &self,
        item: &Self::Item,
        dependencies: &Self::Dependencies,
    ) -> Result<bool, Self::Error> {
        if self.header.block_height > item.header.block_height + 1 {
            return Err(Self::Error {
                details: InvalidBlockErrorReason::BlockOutOfSequence,
            });
        }

        if self.header.block_height <= item.header.block_height {
            return Err(Self::Error {
                details: InvalidBlockErrorReason::NotTallestChain,
            });
        }

        if self.header.block_nonce != item.header.next_block_nonce {
            return Err(Self::Error {
                details: InvalidBlockErrorReason::InvalidBlockNonce,
            });
        }

        if self.header.block_reward.get_amount() != item.header.next_block_reward.get_amount() {
            return Err(Self::Error {
                details: InvalidBlockErrorReason::InvalidBlockReward,
            });
        }

        if let Some((hash, pointers)) = dependencies
            .0
            .get_lowest_pointer(self.header.block_nonce as u128)
        {
            if hash == self.header.claim.hash {
                if let Some(claim_pointer) = self
                    .header
                    .claim
                    .get_pointer(self.header.block_nonce as u128)
                {
                    if pointers != claim_pointer {
                        return Err(Self::Error {
                            details: InvalidBlockErrorReason::InvalidClaimPointers,
                        });
                    }
                } else {
                    return Err(Self::Error {
                        details: InvalidBlockErrorReason::InvalidClaimPointers,
                    });
                }
            } else {
                return Err(Self::Error {
                    details: InvalidBlockErrorReason::InvalidClaimPointers,
                });
            }
        }

        if self.header.last_hash != item.hash {
            return Err(Self::Error {
                details: InvalidBlockErrorReason::InvalidLastHash,
            });
        }

        if self.header.claim.valid(&None, &(None, None)).is_err() {
            return Err(Self::Error {
                details: InvalidBlockErrorReason::InvalidClaim,
            });
        }

        Ok(true)
    }

    fn valid_genesis(&self, _dependencies: &Self::Dependencies) -> Result<bool, Self::Error> {
        let genesis_last_hash = digest("Genesis_Last_Hash".as_bytes());
        let genesis_state_hash = digest(
            format!(
                "{},{}",
                genesis_last_hash,
                digest("Genesis_State_Hash".as_bytes())
            )
            .as_bytes(),
        );

        if self.header.block_height != 0 {
            return Err(Self::Error {
                details: InvalidBlockErrorReason::InvalidBlockHeight,
            });
        }

        if self.header.last_hash != genesis_last_hash {
            return Err(Self::Error {
                details: InvalidBlockErrorReason::InvalidLastHash,
            });
        }

        if self.hash != genesis_state_hash {
            return Err(Self::Error {
                details: InvalidBlockErrorReason::InvalidStateHash,
            });
        }

        if self.header.claim.valid(&None, &(None, None)).is_err() {
            return Err(Self::Error {
                details: InvalidBlockErrorReason::InvalidClaim,
            });
        }

        if self.header.verify().is_err() {
            return Err(Self::Error {
                details: InvalidBlockErrorReason::InvalidBlockSignature,
            });
        }

        let valid_data = true;

        // TODO: Validate threshold signature here for all txns
        // self.txns.iter().for_each(|(_, txn)| {
        //     let n_valid = txn.validators.iter().filter(|(_, &valid)| valid).count();
        //     if (n_valid as f64 / txn.validators.len() as f64) < VALIDATOR_THRESHOLD {
        //         valid_data = false;
        //     }
        // });

        if !valid_data {
            return Err(Self::Error {
                details: InvalidBlockErrorReason::InvalidTxns,
            });
        }

        Ok(true)
    }
}
