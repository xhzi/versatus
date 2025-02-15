use node::{test_utils, test_utils::create_mock_bootstrap_node_config, Node};
use primitives::{generate_account_keypair, Address};
use secp256k1::Message;
use vrrb_core::transactions::NewTransferArgs;
use vrrb_rpc::rpc::{api::RpcApiClient, client::create_client};

#[tokio::test]
#[ignore]
async fn process_full_node_event_flow() {
    let b_node_config = create_mock_bootstrap_node_config();

    let mut bootstrap_node = Node::start(b_node_config).await.unwrap();

    let _bootstrap_gossip_address = bootstrap_node.udp_gossip_address();

    let client = create_client(bootstrap_node.jsonrpc_server_address())
        .await
        .unwrap();

    for _ in 0..1_00 {
        let (sk, pk) = generate_account_keypair();
        let (_, recv_pk) = generate_account_keypair();

        let signature =
            sk.sign_ecdsa(Message::from_hashed_data::<secp256k1::hashes::sha256::Hash>(b"vrrb"));

        client
            .create_txn(NewTransferArgs {
                timestamp: 0,
                sender_address: Address::new(pk),
                sender_public_key: pk,
                receiver_address: Address::new(recv_pk),
                token: None,
                amount: 0,
                signature,
                nonce: 0,
                validators: None,
            })
            .await
            .unwrap();
    }

    let mempool_snapshot = client.get_full_mempool().await.unwrap();

    assert!(!mempool_snapshot.is_empty());

    bootstrap_node.stop();
}
