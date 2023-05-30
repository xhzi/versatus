use std::{net::SocketAddr, thread, thread::sleep, time::Duration};

use async_trait::async_trait;
use crossbeam_channel::{select, unbounded, Sender};
use dkg_engine::{
    dkg::DkgGenerator,
    types::{config::ThresholdConfig, DkgEngine, DkgResult},
};
use events::{Event, Event::PeerRegistration, EventMessage, EventPublisher, SyncPeerData};
use hbbft::crypto::{PublicKey, SecretKeyShare};
use laminar::{Config, Packet, Socket, SocketEvent};
use primitives::{
    NodeIdx,
    NodeType,
    NodeTypeBytes,
    PKShareBytes,
    PayloadBytes,
    QuorumPublicKey,
    QuorumType,
    RawSignature,
    REGISTER_REQUEST,
    RETRIEVE_PEERS_REQUEST,
};
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use telemetry::info;
use theater::{ActorId, ActorLabel, ActorState, Handler, TheaterError};
use tracing::error;

use crate::{result::Result, NodeError};

pub struct DkgModuleConfig {
    pub quorum_type: Option<QuorumType>,
    pub quorum_size: usize,
    pub quorum_threshold: usize,
}

pub struct DkgModule {
    pub dkg_engine: DkgEngine,
    pub quorum_type: Option<QuorumType>,
    pub rendezvous_local_addr: SocketAddr,
    pub rendezvous_server_addr: SocketAddr,
    pub quic_port: u16,
    pub socket: Socket,
    status: ActorState,
    label: ActorLabel,
    id: ActorId,
    broadcast_events_tx: EventPublisher,
}

impl DkgModule {
    pub fn new(
        node_idx: NodeIdx,
        node_type: NodeType,
        secret_key: hbbft::crypto::SecretKey,
        config: DkgModuleConfig,
        rendezvous_local_addr: SocketAddr,
        rendezvous_server_addr: SocketAddr,
        quic_port: u16,
        broadcast_events_tx: EventPublisher,
    ) -> Result<DkgModule> {
        let engine = DkgEngine::new(
            node_idx,
            node_type,
            secret_key,
            ThresholdConfig {
                upper_bound: config.quorum_size as u16,
                threshold: config.quorum_threshold as u16,
            },
        );
        let socket_result = Socket::bind_with_config(
            rendezvous_local_addr,
            Config {
                blocking_mode: false,
                idle_connection_timeout: Duration::from_secs(5),
                heartbeat_interval: None,
                max_packet_size: (16 * 1024) as usize,
                max_fragments: 16_u8,
                fragment_size: 1024,
                fragment_reassembly_buffer_size: 64,
                receive_buffer_max_size: 1452_usize,
                rtt_smoothing_factor: 0.10,
                rtt_max_value: 250,
                socket_event_buffer_size: 1024,
                socket_polling_timeout: Some(Duration::from_millis(1000)),
                max_packets_in_flight: 512,
                max_unestablished_connections: 50,
            },
        );
        match socket_result {
            Ok(socket) => Ok(Self {
                dkg_engine: engine,
                quorum_type: config.quorum_type,
                rendezvous_local_addr,
                rendezvous_server_addr,
                quic_port,
                socket,
                status: ActorState::Stopped,
                label: String::from("State"),
                id: uuid::Uuid::new_v4().to_string(),
                broadcast_events_tx,
            }),
            Err(e) => Err(NodeError::Other(format!(
                "Error occurred while binding socket to port. Details :{0}",
                e
            ))),
        }
    }

    #[cfg(test)]
    pub fn make_engine(
        dkg_engine: DkgEngine,
        _events_tx: EventPublisher,
        broadcast_events_tx: EventPublisher,
    ) -> Self {
        use std::net::{IpAddr, Ipv4Addr};

        let socket = Socket::bind_with_config(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0),
            Config {
                blocking_mode: false,
                idle_connection_timeout: Duration::from_secs(5),
                heartbeat_interval: None,
                max_packet_size: (16 * 1024) as usize,
                max_fragments: 16_u8,
                fragment_size: 1024,
                fragment_reassembly_buffer_size: 64,
                receive_buffer_max_size: 1452_usize,
                rtt_smoothing_factor: 0.10,
                rtt_max_value: 250,
                socket_event_buffer_size: 1024,
                socket_polling_timeout: Some(Duration::from_millis(1000)),
                max_packets_in_flight: 512,
                max_unestablished_connections: 50,
            },
        )
        .unwrap();
        Self {
            dkg_engine,
            quorum_type: Some(QuorumType::Farmer),
            rendezvous_local_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0),
            rendezvous_server_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0),
            quic_port: 9090,
            socket,
            status: ActorState::Stopped,
            label: String::from("State"),
            id: uuid::Uuid::new_v4().to_string(),
            broadcast_events_tx,
        }
    }

    fn name(&self) -> String {
        String::from("DKG module")
    }

    fn generate_random_payload(&self, secret_key_share: &SecretKeyShare) -> (Vec<u8>, Vec<u8>) {
        let message: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(15)
            .map(char::from)
            .collect();

        let msg_bytes = if let Ok(m) = hex::decode(message.clone()) {
            m
        } else {
            vec![]
        };

        let signature = secret_key_share.sign(message).to_bytes().to_vec();

        (msg_bytes, signature)
    }

    fn spawn_interval_thread(interval: Duration, tx: Sender<()>) {
        thread::spawn(move || loop {
            sleep(interval);
            let _ = tx.send(());
        });
    }
}

#[async_trait]
impl Handler<EventMessage> for DkgModule {
    fn id(&self) -> ActorId {
        self.id.clone()
    }

    fn label(&self) -> ActorLabel {
        self.name()
    }

    fn status(&self) -> ActorState {
        self.status.clone()
    }

    fn set_status(&mut self, actor_status: ActorState) {
        self.status = actor_status;
    }

    async fn handle(&mut self, event: EventMessage) -> theater::Result<ActorState> {
        match event.into() {
            Event::Stop => {
                return Ok(ActorState::Stopped);
            },
            Event::DkgInitiate => {
                let threshold_config = self.dkg_engine.threshold_config.clone();
                if self.quorum_type.clone().is_some() {
                    match self
                        .dkg_engine
                        .generate_sync_keygen_instance(threshold_config.threshold as usize)
                    {
                        Ok(part_commitment) => {
                            if let DkgResult::PartMessageGenerated(node_idx, part) = part_commitment
                            {
                                if let Ok(part_committment_bytes) = bincode::serialize(&part) {
                                    let _ = self
                                        .broadcast_events_tx
                                        .send(
                                            Event::PartMessage(node_idx, part_committment_bytes)
                                                .into(),
                                        )
                                        .await.map_err(|e| {
                                            error!("Error occured while sending part message to broadcast event channel {:?}", e);
                                            TheaterError::Other(format!("{:?}", e))
                                        });
                                }
                            }
                        },
                        Err(_e) => {
                            error!("Error occured while generating synchronized keygen instance for node {:?}", self.dkg_engine.node_idx);
                        },
                    }
                } else {
                    error!(
                        "Cannot participate into DKG ,since current node {:?} dint win any Quorum Election",
                        self.dkg_engine.node_idx
                    );
                }
                return Ok(ActorState::Running);
            },
            Event::PartMessage(node_idx, part_committment_bytes) => {
                let part: bincode::Result<hbbft::sync_key_gen::Part> =
                    bincode::deserialize(&part_committment_bytes);
                if let Ok(part_committment) = part {
                    self.dkg_engine
                        .dkg_state
                        .part_message_store
                        .entry(node_idx)
                        .or_insert_with(|| part_committment);
                };
            },
            Event::AckPartCommitment(sender_id) => {
                if self
                    .dkg_engine
                    .dkg_state
                    .part_message_store
                    .contains_key(&sender_id)
                {
                    let dkg_result = self.dkg_engine.ack_partial_commitment(sender_id);
                    match dkg_result {
                        Ok(status) => match status {
                            DkgResult::PartMessageAcknowledged => {
                                if let Some(ack) = self
                                    .dkg_engine
                                    .dkg_state
                                    .ack_message_store
                                    .get(&(sender_id, self.dkg_engine.node_idx))
                                {
                                    if let Ok(ack_bytes) = bincode::serialize(&ack) {
                                        let event = Event::SendAck(
                                            self.dkg_engine.node_idx,
                                            sender_id,
                                            ack_bytes,
                                        );

                                        let _ = self.broadcast_events_tx.send(event.into()).await.map_err(|e| {
                                            error!("Error occured while sending ack message to broadcast event channel {:?}", e);
                                            TheaterError::Other(format!("{:?}", e))
                                        });
                                    };
                                }
                            },
                            _ => {
                                error!("Error occured while acknowledging partial commitment for node {:?}", sender_id,);
                            },
                        },
                        Err(err) => {
                            error!("Error occured while acknowledging partial commitment for node {:?}: Err {:?}", sender_id, err);
                        },
                    }
                } else {
                    error!("Part Committment for Node idx {:?} missing ", sender_id);
                }
            },
            Event::HandleAllAcks => {
                let result = self.dkg_engine.handle_ack_messages();
                match result {
                    Ok(status) => {
                        info!("DKG Handle All Acks status {:?}", status);
                    },
                    Err(e) => {
                        error!("Error occured while handling all the acks {:?}", e);
                    },
                }
            },
            Event::GenerateKeySet => {
                let result = self.dkg_engine.generate_key_sets();
                match result {
                    Ok(status) => {
                        info!("DKG Completion status {:?}", status);
                        if let Some(public_key_set) =
                            self.dkg_engine.dkg_state.public_key_set.as_ref()
                        {
                            let _ = self
                                .broadcast_events_tx
                                .send(EventMessage::new(
                                    None,
                                    Event::QuorumKey(
                                        public_key_set.public_key().to_bytes().to_vec(),
                                    ),
                                ))
                                .await;

                            //Namespace Registration
                            let _ = self
                                .broadcast_events_tx
                                .send(EventMessage::new(
                                    None,
                                    Event::NamespaceRegistration(
                                        self.dkg_engine.node_type.clone(),
                                        public_key_set.public_key().to_bytes().to_vec(),
                                    ),
                                ))
                                .await;
                        }
                    },
                    Err(e) => {
                        error!("Error occurred while generating Quorum Public Key {:?}", e);
                    },
                }
            },
            Event::HarvesterPublicKey(key_bytes) => {
                let result: bincode::Result<PublicKey> = bincode::deserialize(&key_bytes);
                if let Ok(harvester_public_key) = result {
                    self.dkg_engine.harvester_public_key = Some(harvester_public_key);
                }
            },
            Event::GeneratePayloadForPeerRegistration => {
                if let Some(secret_key_share) = self.dkg_engine.dkg_state.secret_key_share.clone() {
                    let (msg_bytes, signature) = self.generate_random_payload(&secret_key_share);
                    if let Some(pk) = self.dkg_engine.dkg_state.public_key_set.clone() {
                        if let Some(secret_key_share) =
                            self.dkg_engine.dkg_state.secret_key_share.clone()
                        {
                            let _ = self
                                .broadcast_events_tx
                                .send(EventMessage::new(
                                    None,
                                    PeerRegistration(
                                        secret_key_share.public_key_share().to_bytes().to_vec(),
                                        pk.public_key().to_bytes().to_vec(),
                                        msg_bytes,
                                        signature,
                                        self.dkg_engine.node_type,
                                        self.quic_port,
                                    ),
                                ))
                                .await;
                        }
                    }
                }
            },
            Event::NoOp => {},
            _ => {},
        }

        Ok(ActorState::Running)
    }

    fn on_stop(&self) {
        info!(
            "{}-{} received stop signal. Stopping",
            self.name(),
            self.label()
        );
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use dkg_engine::test_utils;
    use events::{Event, DEFAULT_BUFFER};
    use hbbft::crypto::SecretKey;
    use primitives::{NodeType, QuorumType::Farmer};
    use theater::{Actor, ActorImpl};

    use super::*;

    #[tokio::test]
    async fn dkg_runtime_module_starts_and_stops() {
        let (broadcast_events_tx, _broadcast_events_rx) =
            tokio::sync::mpsc::channel::<EventMessage>(DEFAULT_BUFFER);

        let (_events_tx, _) = tokio::sync::mpsc::unbounded_channel::<Event>();
        let dkg_config = DkgModuleConfig {
            quorum_type: Some(Farmer),
            quorum_size: 4,
            quorum_threshold: 2,
        };
        let sec_key: SecretKey = SecretKey::random();
        let dkg_module = DkgModule::new(
            1,
            NodeType::MasterNode,
            sec_key,
            dkg_config,
            "127.0.0.1:3031".parse().unwrap(),
            "127.0.0.1:3030".parse().unwrap(),
            9092,
            broadcast_events_tx,
        )
        .unwrap();

        let mut dkg_module = ActorImpl::new(dkg_module);

        let (ctrl_tx, mut ctrl_rx) =
            tokio::sync::broadcast::channel::<EventMessage>(DEFAULT_BUFFER);

        assert_eq!(dkg_module.status(), ActorState::Stopped);

        let handle = tokio::spawn(async move {
            dkg_module.start(&mut ctrl_rx).await.unwrap();
            assert_eq!(dkg_module.status(), ActorState::Terminating);
        });

        ctrl_tx.send(Event::Stop.into()).unwrap();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn dkg_runtime_dkg_init() {
        let (broadcast_events_tx, mut broadcast_events_rx) =
            tokio::sync::mpsc::channel::<EventMessage>(DEFAULT_BUFFER);

        let (_events_tx, _) = tokio::sync::mpsc::unbounded_channel::<Event>();
        let dkg_config = DkgModuleConfig {
            quorum_type: Some(Farmer),
            quorum_size: 4,
            quorum_threshold: 2,
        };
        let sec_key: SecretKey = SecretKey::random();
        let mut dkg_module = DkgModule::new(
            1,
            NodeType::MasterNode,
            sec_key.clone(),
            dkg_config,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0),
            9091,
            broadcast_events_tx,
        )
        .unwrap();
        dkg_module
            .dkg_engine
            .add_peer_public_key(1, sec_key.public_key());
        dkg_module
            .dkg_engine
            .add_peer_public_key(2, SecretKey::random().public_key());
        dkg_module
            .dkg_engine
            .add_peer_public_key(3, SecretKey::random().public_key());
        dkg_module
            .dkg_engine
            .add_peer_public_key(4, SecretKey::random().public_key());
        let mut dkg_module = ActorImpl::new(dkg_module);

        let (ctrl_tx, mut ctrl_rx) =
            tokio::sync::broadcast::channel::<EventMessage>(DEFAULT_BUFFER);

        assert_eq!(dkg_module.status(), ActorState::Stopped);

        let handle = tokio::spawn(async move {
            dkg_module.start(&mut ctrl_rx).await.unwrap();
            assert_eq!(dkg_module.status(), ActorState::Terminating);
        });

        ctrl_tx.send(Event::DkgInitiate.into()).unwrap();
        ctrl_tx.send(Event::AckPartCommitment(1).into()).unwrap();
        ctrl_tx.send(Event::Stop.into()).unwrap();

        let part_message_event = broadcast_events_rx.recv().await.unwrap();
        match part_message_event.into() {
            Event::PartMessage(_, part_committment_bytes) => {
                let part_committment: bincode::Result<hbbft::sync_key_gen::Part> =
                    bincode::deserialize(&part_committment_bytes);
                assert!(part_committment.is_ok());
            },
            _ => {},
        }

        handle.await.unwrap();
    }

    #[tokio::test]
    async fn dkg_runtime_dkg_ack() {
        let (broadcast_events_tx, mut broadcast_events_rx) =
            tokio::sync::mpsc::channel::<EventMessage>(DEFAULT_BUFFER);

        let (_events_tx, _) = tokio::sync::mpsc::unbounded_channel::<Event>();
        let dkg_config = DkgModuleConfig {
            quorum_type: Some(Farmer),
            quorum_size: 4,
            quorum_threshold: 2,
        };
        let sec_key: SecretKey = SecretKey::random();
        let mut dkg_module = DkgModule::new(
            1,
            NodeType::MasterNode,
            sec_key.clone(),
            dkg_config,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0),
            9092,
            broadcast_events_tx.clone(),
        )
        .unwrap();

        dkg_module
            .dkg_engine
            .add_peer_public_key(1, sec_key.public_key());

        dkg_module
            .dkg_engine
            .add_peer_public_key(2, SecretKey::random().public_key());

        dkg_module
            .dkg_engine
            .add_peer_public_key(3, SecretKey::random().public_key());

        dkg_module
            .dkg_engine
            .add_peer_public_key(4, SecretKey::random().public_key());

        let _node_idx = dkg_module.dkg_engine.node_idx;
        let mut dkg_module = ActorImpl::new(dkg_module);

        let (ctrl_tx, mut ctrl_rx) = tokio::sync::broadcast::channel::<EventMessage>(20);

        assert_eq!(dkg_module.status(), ActorState::Stopped);

        let handle = tokio::spawn(async move {
            dkg_module.start(&mut ctrl_rx).await.unwrap();
            assert_eq!(dkg_module.status(), ActorState::Terminating);
        });

        ctrl_tx.send(Event::DkgInitiate.into()).unwrap();

        let msg = broadcast_events_rx.recv().await.unwrap();
        if let Event::PartMessage(sender_id, part) = msg.into() {
            assert_eq!(sender_id, 1);
            assert!(!part.is_empty());
        }
        ctrl_tx.send(Event::AckPartCommitment(1).into()).unwrap();

        let msg1 = broadcast_events_rx.recv().await.unwrap();

        if let Event::SendAck(curr_id, sender_id, ack) = msg1.into() {
            assert_eq!(curr_id, 1);
            assert_eq!(sender_id, 1);
            assert!(!ack.is_empty());
        }

        ctrl_tx.send(Event::Stop.into()).unwrap();

        handle.await.unwrap();
    }

    #[tokio::test]
    async fn dkg_runtime_handle_all_acks_generate_keyset() {
        let mut dkg_engines = test_utils::generate_dkg_engine_with_states().await;
        let (events_tx, _) = tokio::sync::mpsc::channel::<EventMessage>(DEFAULT_BUFFER);
        let (broadcast_events_tx, _broadcast_events_rx) =
            tokio::sync::mpsc::channel::<EventMessage>(DEFAULT_BUFFER);

        let dkg_module =
            DkgModule::make_engine(dkg_engines.pop().unwrap(), events_tx, broadcast_events_tx);

        let mut dkg_module = ActorImpl::new(dkg_module);

        let (ctrl_tx, mut ctrl_rx) = tokio::sync::broadcast::channel::<EventMessage>(20);

        assert_eq!(dkg_module.status(), ActorState::Stopped);

        let handle = tokio::spawn(async move {
            dkg_module.start(&mut ctrl_rx).await.unwrap();
            assert_eq!(dkg_module.status(), ActorState::Terminating);
        });

        ctrl_tx.send(Event::HandleAllAcks.into()).unwrap();
        ctrl_tx.send(Event::GenerateKeySet.into()).unwrap();
        ctrl_tx.send(Event::Stop.into()).unwrap();

        handle.await.unwrap();
    }
}
