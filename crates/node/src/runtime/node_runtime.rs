use std::{
    collections::HashMap,
    fmt::format,
    hash::Hash,
    sync::{Arc, RwLock},
};

use block::{
    header::BlockHeader, vesting::GenesisConfig, Block, Certificate, ClaimHash, ConvergenceBlock,
    GenesisBlock, ProposalBlock, RefHash,
};
use bulldag::graph::BullDag;
use dkg_engine::prelude::{DkgEngine, DkgEngineConfig, ReceiverId, SenderId};
use ethereum_types::U256;
use events::{AssignedQuorumMembership, EventPublisher, PeerData};
use hbbft::sync_key_gen::{Ack, Part};
use mempool::{LeftRightMempool, MempoolReadHandleFactory, TxnRecord};
use miner::{Miner, MinerConfig};
use primitives::{
    Address, Epoch, NodeId, NodeType, PublicKey, QuorumKind, Round, ValidatorPublicKey,
};
use ritelinked::LinkedHashMap;
use secp256k1::Message;
use storage::vrrbdb::{ApplyBlockResult, VrrbDbConfig, VrrbDbReadHandle};
use theater::{ActorId, ActorState};
use tokio::task::JoinHandle;
use utils::payload::digest_data_to_bytes;
use vrrb_config::{NodeConfig, QuorumMembershipConfig};
use vrrb_core::{
    account::{Account, UpdateArgs},
    claim::Claim,
    transactions::{
        generate_transfer_digest_vec, NewTransferArgs, Token, Transaction, TransactionDigest,
        TransactionKind, Transfer,
    },
};

use crate::{
    consensus::{ConsensusModule, ConsensusModuleConfig},
    mining_module::{MiningModule, MiningModuleConfig},
    result::{NodeError, Result},
    state_manager::{StateManager, StateManagerConfig},
};

pub const PULL_TXN_BATCH_SIZE: usize = 100;

#[derive(Debug, Clone)]
pub struct NodeRuntime {
    // TODO: reduce scope visibility of these
    pub id: ActorId,
    pub status: ActorState,
    // TODO: make private
    pub config: NodeConfig,
    pub events_tx: EventPublisher,
    pub state_driver: StateManager,
    pub consensus_driver: ConsensusModule,
    pub mining_driver: Miner,
}

impl NodeRuntime {
    pub async fn new(config: &NodeConfig, events_tx: EventPublisher) -> Result<Self> {
        let dag: Arc<RwLock<BullDag<Block, String>>> = Arc::new(RwLock::new(BullDag::new()));

        let miner_public_key = config.keypair.get_miner_public_key().to_owned();

        let signature = Claim::signature_for_valid_claim(
            miner_public_key,
            config.public_ip_address,
            config
                .keypair
                .get_miner_secret_key()
                .secret_bytes()
                .to_vec(),
        )?;

        let claim = Claim::new(
            miner_public_key,
            Address::new(miner_public_key),
            config.public_ip_address,
            signature,
            config.id.clone(),
        )
        .map_err(NodeError::from)?;

        let mut vrrbdb_config = VrrbDbConfig::default();

        if config.db_path() != &vrrbdb_config.path {
            vrrbdb_config.with_path(config.db_path().to_path_buf());
        }

        let database = storage::vrrbdb::VrrbDb::new(vrrbdb_config);
        let mempool = LeftRightMempool::new();

        let state_driver = StateManager::new(StateManagerConfig {
            database,
            mempool,
            dag,
            claim,
        });

        let dag: Arc<RwLock<BullDag<Block, String>>> = Arc::new(RwLock::new(BullDag::new()));

        let (_, miner_secret_key) = config.keypair.get_secret_keys();
        let (_, miner_public_key) = config.keypair.get_public_keys();

        let miner_config = MinerConfig {
            secret_key: *miner_secret_key,
            public_key: *miner_public_key,
            ip_address: config.public_ip_address,
            dag,
        };

        let miner = miner::Miner::new(miner_config, config.id.clone()).map_err(NodeError::from)?;

        let dkg_engine_config = DkgEngineConfig {
            node_id: config.id.clone(),
            node_type: config.node_type,
            secret_key: config.keypair.get_validator_secret_key_owned(),
            threshold_config: config.threshold_config.clone(),
        };

        let dkg_generator = DkgEngine::new(dkg_engine_config);

        let consensus_driver = ConsensusModule::new(ConsensusModuleConfig {
            keypair: config.keypair.clone(),
            node_config: config.clone(),
            dkg_generator,
            validator_public_key: config.keypair.validator_public_key_owned(),
        });

        Ok(Self {
            id: uuid::Uuid::new_v4().to_string(),
            status: ActorState::Stopped,
            config: config.to_owned(),
            state_driver,
            consensus_driver,
            events_tx,
            mining_driver: miner,
        })
    }

    pub fn config_ref(&self) -> &NodeConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut NodeConfig {
        &mut self.config
    }

    pub fn config_owned(&self) -> NodeConfig {
        self.config.clone()
    }

    fn _setup_reputation_module() -> Result<Option<JoinHandle<Result<()>>>> {
        Ok(None)
    }

    fn _setup_credit_model_module() -> Result<Option<JoinHandle<Result<()>>>> {
        Ok(None)
    }

    pub fn has_required_node_type(&self, intended_node_type: NodeType, action: &str) -> Result<()> {
        if !matches!(self.config.node_type, intended_node_type) {
            return Err(NodeError::Other(format!(
                "Only {intended_node_type} nodes are allowed to: {action}"
            )));
        }
        Ok(())
    }

    pub fn belongs_to_correct_quorum(
        &self,
        intended_quorum: QuorumKind,
        action: &str,
    ) -> Result<()> {
        if let Some(membership) = self.quorum_membership() {
            let quorum_kind = membership.quorum_kind();

            if !matches!(quorum_kind, intended_quorum) {
                return Err(NodeError::Other(format!(
                    "Only {intended_quorum} nodes are allowed to: {action}"
                )));
            }
        } else {
            return Err(NodeError::Other(format!(
                "No quorum configuration found for node"
            )));
        }

        Ok(())
    }

    pub fn quorum_membership(&self) -> Option<QuorumMembershipConfig> {
        self.consensus_driver
            .quorum_driver
            .membership_config
            .clone()
    }

    pub fn state_read_handle(&self) -> VrrbDbReadHandle {
        self.state_driver.read_handle()
    }

    pub fn mempool_read_handle_factory(&self) -> MempoolReadHandleFactory {
        self.state_driver.mempool_read_handle_factory()
    }

    pub fn mempool_snapshot(&self) -> HashMap<TransactionDigest, TxnRecord> {
        self.mempool_read_handle_factory().entries()
    }

    pub fn add_peer_public_key_to_dkg_state(
        &mut self,
        node_id: NodeId,
        public_key: ValidatorPublicKey,
    ) {
        self.consensus_driver
            .add_peer_public_key_to_dkg_state(node_id, public_key);
    }

    pub fn generate_partial_commitment_message(&mut self) -> Result<(Part, NodeId)> {
        let (part, node_id) = self
            .consensus_driver
            .generate_partial_commitment_message()?;

        // self.store_part_commitment(node_id.clone(), part.clone());

        Ok((part, node_id))
    }

    pub fn store_part_commitment(&mut self, node_id: NodeId, part: Part) {
        self.consensus_driver
            .dkg_engine
            .dkg_state
            .part_message_store_mut()
            .entry(node_id)
            .or_insert_with(|| part);
    }
    pub fn generate_keysets(&mut self) -> Result<()> {
        self.consensus_driver.generate_keysets()
    }

    pub fn produce_genesis_transactions(
        &self,
    ) -> Result<LinkedHashMap<TransactionDigest, TransactionKind>> {
        self.has_required_node_type(NodeType::Bootstrap, "produce genesis transactions")?;

        let sender_public_key = self.config.keypair.miner_public_key_owned();
        let address = Address::new(sender_public_key);

        let txns = block::vesting::generate_genesis_txns(GenesisConfig::new(address.clone()));

        Ok(txns)
    }

    pub fn mine_genesis_block(
        &self,
        txns: LinkedHashMap<TransactionDigest, TransactionKind>,
    ) -> Result<GenesisBlock> {
        self.has_required_node_type(NodeType::Miner, "mine genesis block")?;

        let claim = self.state_driver.dag.claim();

        let claim_list = vec![(claim.hash, claim.clone())];

        let claim_list_hash = digest_data_to_bytes(&claim_list);
        let seed = 0;
        let round = 0;
        let epoch = 0;

        let header = BlockHeader::genesis(
            seed,
            round,
            epoch,
            claim.clone(),
            self.config.keypair.miner_secret_key_owned(),
            hex::encode(claim_list_hash),
        );

        let block_header = header.clone();
        let block_hash = digest_data_to_bytes(&(
            header.ref_hashes,
            header.round,
            header.block_seed,
            header.next_block_seed,
            header.block_height,
            header.timestamp,
            header.txn_hash,
            header.miner_claim,
            header.claim_list_hash,
            header.block_reward,
            header.next_block_reward,
            header.miner_signature,
        ));

        let mut claims = LinkedHashMap::new();
        claims.insert(claim.hash, claim);

        let genesis = GenesisBlock {
            header: block_header,
            txns,
            claims,
            hash: hex::encode(block_hash),
            certificate: None,
        };

        Ok(genesis)
    }

    pub fn mine_convergence_block(&mut self) -> Result<ConvergenceBlock> {
        self.has_required_node_type(NodeType::Miner, "mine convergence block")?;
        self.mining_driver
            .mine_convergence_block()
            .ok_or(NodeError::Other(
                "Could not mine convergence block".to_string(),
            ))
    }

    pub fn certify_convergence_block(&mut self, block: ConvergenceBlock) -> Result<()> {
        self.has_required_node_type(NodeType::Validator, "certify convergence block")?;
        self.belongs_to_correct_quorum(QuorumKind::Harvester, "certify convergence block")?;

        let last_block_header =
            self.state_driver
                .dag
                .last_confirmed_block_header()
                .ok_or(NodeError::Other(format!(
                    "Node {} does not have a last confirmed block header",
                    self.config.id
                )))?;

        self.consensus_driver
            .certify_convergence_block(block, last_block_header);

        Ok(())
    }

    pub fn transactions_root_hash(&self) -> Result<String> {
        self.state_driver.transactions_root_hash()
    }

    pub fn state_root_hash(&self) -> Result<String> {
        self.state_driver.state_root_hash()
    }

    pub fn state_snapshot(&self) -> HashMap<Address, Account> {
        let handle = self.state_driver.read_handle();
        handle.state_store_values()
    }

    pub fn transactions_snapshot(&self) -> HashMap<TransactionDigest, TransactionKind> {
        let handle = self.state_driver.read_handle();
        handle.transaction_store_values()
    }

    pub fn claims_snapshot(&self) -> HashMap<NodeId, Claim> {
        let handle = self.state_driver.read_handle();
        handle.claim_store_values()
    }

    async fn get_transaction_by_id(
        &self,
        transaction_digest: TransactionDigest,
    ) -> Result<TransactionKind> {
        todo!()
    }

    pub fn create_account(&mut self, public_key: PublicKey) -> Result<Address> {
        let account = Account::new(public_key);
        let address = Address::new(public_key);

        self.state_driver.insert_account(address.clone(), account)?;

        Ok(address)
    }

    pub fn update_account(&mut self, args: UpdateArgs) -> Result<()> {
        self.state_driver.update_account(args)
    }

    pub fn get_account_by_address(&self, address: &Address) -> Result<Account> {
        self.state_driver.get_account(address)
    }

    pub fn get_round(&self) -> Result<Round> {
        let header =
            self.state_driver
                .dag
                .last_confirmed_block_header()
                .ok_or(NodeError::Other(format!(
                    "failed to fetch latest block header from dag"
                )))?;

        Ok(header.round)
    }

    pub fn get_claims_by_account_address(&self, address: &Address) -> Result<Vec<Claim>> {
        self.state_driver.get_claims_by_account_address(address)
    }

    pub fn get_claim_hashes(&self) -> Result<Vec<ClaimHash>> {
        todo!()
    }

    pub fn get_claims(&self, claim_hashes: Vec<ClaimHash>) -> Result<Vec<Claim>> {
        self.state_driver.get_claims(claim_hashes)
    }
}

impl NodeRuntime {
    pub fn handle_block_received(&mut self, block: Block) -> Result<ApplyBlockResult> {
        match block {
            Block::Genesis { block } => self.handle_genesis_block_received(block),
            Block::Proposal { block } => self.handle_proposal_block_received(block),
            Block::Convergence { block } => self.handle_convergence_block_received(block),
        }
    }

    fn handle_genesis_block_received(&mut self, block: GenesisBlock) -> Result<ApplyBlockResult> {
        self.has_required_node_type(NodeType::Validator, "store genesis block")?;
        self.belongs_to_correct_quorum(QuorumKind::Harvester, "store genesis block")?;

        self.state_driver
            .dag
            .append_genesis(&block)
            .map_err(|err| {
                NodeError::Other(format!("Failed to append genesis block to DAG: {err:?}"))
            })?;

        let apply_result = self.state_driver.apply_block(Block::Genesis { block })?;

        Ok(apply_result)
    }

    fn handle_proposal_block_received(&mut self, block: ProposalBlock) -> Result<ApplyBlockResult> {
        if let Err(e) = self.state_driver.dag.append_proposal(&block) {
            let err_note = format!("Failed to append proposal block to DAG: {e:?}");
            return Err(NodeError::Other(err_note));
        }
        todo!()
    }

    /// Certifies and stores a convergence block within a node's state if certification succeeds
    fn handle_convergence_block_received(
        &mut self,
        block: ConvergenceBlock,
    ) -> Result<ApplyBlockResult> {
        self.has_required_node_type(NodeType::Validator, "certify convergence block")?;
        self.belongs_to_correct_quorum(QuorumKind::Harvester, "certify convergence block")?;

        self.state_driver
            .dag
            .append_convergence(&block)
            .map_err(|err| {
                NodeError::Other(format!(
                    "Could not append convergence block to DAG: {err:?}"
                ))
            })?;

        if block.certificate.is_none() {
            if let Some(header) = self.state_driver.dag.last_confirmed_block_header() {
                self.consensus_driver
                    .certify_convergence_block(block.clone(), header);
            }
        }

        let apply_result = self
            .state_driver
            .apply_block(Block::Convergence { block })?;

        Ok(apply_result)
    }

    pub fn handle_block_certificate_created(&mut self, certificate: Certificate) -> Result<()> {
        //
        //         let mut mine_block: Option<ConvergenceBlock> = None;
        //         let block_hash = certificate.block_hash.clone();
        //         if let Ok(Some(Block::Convergence { mut block })) =
        //             self.dag.write().map(|mut bull_dag| {
        //                 bull_dag
        //                     .get_vertex_mut(block_hash)
        //                     .map(|vertex| vertex.get_data())
        //             })
        //         {
        //             block.append_certificate(certificate.clone());
        //             self.last_confirmed_block_header = Some(block.get_header());
        //             mine_block = Some(block.clone());
        //         }
        //         if let Some(block) = mine_block {
        //             let proposal_block = Event::MineProposalBlock(
        //                 block.hash.clone(),
        //                 block.get_header().round,
        //                 block.get_header().epoch,
        //                 self.claim.clone(),
        //             );
        //             if let Err(err) = self
        //                 .events_tx
        //                 .send(EventMessage::new(None, proposal_block.clone()))
        //                 .await
        //             {
        //                 let err_msg = format!(
        //                     "Error occurred while broadcasting event {proposal_block:?}: {err:?}"
        //                 );
        //                 return Err(TheaterError::Other(err_msg));
        //             }
        //         } else {
        //             telemetry::debug!("Missing ConvergenceBlock for certificate: {certificate:?}");
        //         }
        //
        todo!()
    }

    pub async fn handle_node_added_to_peer_list(
        &mut self,
        peer_data: PeerData,
    ) -> Result<Option<HashMap<NodeId, AssignedQuorumMembership>>> {
        self.consensus_driver
            .handle_node_added_to_peer_list(peer_data)
            .await
    }

    pub fn handle_proposal_block_mine_request_created(
        &mut self,
        ref_hash: RefHash,
        round: Round,
        epoch: Epoch,
        claim: Claim,
    ) -> Result<ProposalBlock> {
        self.has_required_node_type(NodeType::Validator, "create proposal block")?;
        self.belongs_to_correct_quorum(QuorumKind::Harvester, "create proposal block")?;

        // let proposal_block = self
        //     .consensus_driver
        //     .handle_proposal_block_mine_request_created(
        //         args.ref_hash,
        //         args.round,
        //         args.epoch,
        //         args.claim,
        //     )?;
        //
        // Ok(proposal_block)
        todo!()
    }

    pub fn handle_part_commitment_created(
        &mut self,
        sender_id: SenderId,
        part: Part,
    ) -> Result<(ReceiverId, SenderId, Ack)> {
        self.consensus_driver
            .handle_part_commitment_created(sender_id, part)
    }

    pub fn handle_part_commitment_acknowledged(
        &mut self,
        receiver_id: ReceiverId,
        sender_id: SenderId,
        ack: Ack,
    ) -> Result<()> {
        self.consensus_driver
            .handle_part_commitment_acknowledged(receiver_id, sender_id, ack)
    }
    pub fn handle_all_ack_messages(&mut self) -> Result<()> {
        self.consensus_driver.handle_all_ack_messages()
    }

    pub fn handle_quorum_membership_assigment_created(
        &mut self,
        assigned_membership: AssignedQuorumMembership,
    ) -> Result<()> {
        self.consensus_driver
            .handle_quorum_membership_assigment_created(assigned_membership)
    }
    pub fn handle_convergence_block_precheck_requested(
        &mut self,
        block: ConvergenceBlock,
        last_confirmed_block_header: BlockHeader,
    ) {
        self.consensus_driver
            .precheck_convergence_block(block, last_confirmed_block_header);
    }
}
