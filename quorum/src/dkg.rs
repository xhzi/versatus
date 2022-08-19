use rand::prelude::*;
use rand::RngCore;
use rand_core::OsRng;
use std::{collections::BTreeMap, sync::Arc};

use hbbft::crypto::{PublicKey, SecretKey};
use hbbft::sync_key_gen::{AckOutcome, PartOutcome, SyncKeyGen};
use threshold_crypto::SignatureShare;

// fn run() {
//     //
//     // Use the OS random number generator for any randomness:
//     // let mut rng = OsRng::new().expect("Could not open OS random number generator.");
//     let mut rng = OsRng::default();
//
//     // Two out of four shares will suffice to sign or encrypt something.
//     let (threshold, node_num) = (1, 4);
//
//     // Generate individual key pairs for encryption. These are not suitable for threshold schemes.
//     // let sec_keys: Vec<SecretKey> = (0..node_num).map(|_| rand::random()).collect();
//     // let sec_keys: Vec<SecretKey> = (0..node_num).map(|_| rng.gen()).collect();
//     // let sec_keys: Vec<SecretKey> = (0..node_num).map(|_| SecretKey::default()).collect();
//     let sec_keys: Vec<SecretKey> = (0..node_num).map(|_| SecretKey::random()).collect();
//
//     let pub_keys: BTreeMap<usize, PublicKey> = sec_keys
//         .iter()
//         .map(SecretKey::public_key)
//         .enumerate()
//         .collect();
//
//     let pub_keys = Arc::new(pub_keys);
//
//     // Create the `SyncKeyGen` instances. The constructor also outputs the part that needs to
//     // be sent to all other participants, so we save the parts together with their sender ID.
//     let mut nodes = BTreeMap::new();
//     let mut parts = Vec::new();
//
//     for (id, sk) in sec_keys.into_iter().enumerate() {
//         let (sync_key_gen, opt_part) = SyncKeyGen::new(
//             //
//             id,
//             sk,
//             pub_keys.clone(),
//             threshold,
//             &mut rng,
//         )
//         .unwrap_or_else(|_| panic!("Failed to create `SyncKeyGen` instance for node #{}", id));
//
//         nodes.insert(id, sync_key_gen);
//         parts.push((id, opt_part.unwrap())); // Would be `None` for observer nodes.
//     }
//
//     /*
//
//     // All nodes now handle the parts and send the resulting `Ack` messages.
//     let mut acks = Vec::new();
//     for (sender_id, part) in parts {
//         for (&id, node) in &mut nodes {
//             match node
//                 .handle_part(&sender_id, part.clone(), &mut rng)
//                 .expect("Failed to handle Part")
//             {
//                 PartOutcome::Valid(Some(ack)) => acks.push((id, ack)),
//                 PartOutcome::Invalid(fault) => panic!("Invalid Part: {:?}", fault),
//                 PartOutcome::Valid(None) => {
//                     panic!("We are not an observer, so we should send Ack.")
//                 }
//             }
//         }
//     }
//
//     // Finally, we handle all the `Ack`s.
//     for (sender_id, ack) in acks {
//         for node in nodes.values_mut() {
//             match node
//                 .handle_ack(&sender_id, ack.clone())
//                 .expect("Failed to handle Ack")
//             {
//                 AckOutcome::Valid => (),
//                 AckOutcome::Invalid(fault) => panic!("Invalid Ack: {:?}", fault),
//             }
//         }
//     }
//
//     // We have all the information and can generate the key sets.
//     // Generate the public key set; which is identical for all nodes.
//     let pub_key_set = nodes[&0]
//         .generate()
//         .expect("Failed to create `PublicKeySet` from node #0")
//         .0;
//
//     let mut secret_key_shares = BTreeMap::new();
//     for (&id, node) in &mut nodes {
//         assert!(node.is_ready());
//         let (pks, opt_sks) = node.generate().unwrap_or_else(|_| {
//             panic!(
//                 "Failed to create `PublicKeySet` and `SecretKeyShare` for node #{}",
//                 id
//             )
//         });
//         assert_eq!(pks, pub_key_set); // All nodes now know the public keys and public key shares.
//         let sks = opt_sks.expect("Not an observer node: We receive a secret key share.");
//         secret_key_shares.insert(id, sks);
//     }
//
//     // Two out of four nodes can now sign a message. Each share can be verified individually.
//     let msg = "Nodes 0 and 1 does not agree with this.";
//     let mut sig_shares: BTreeMap<usize, SignatureShare> = BTreeMap::new();
//     for (&id, sks) in &secret_key_shares {
//         if id != 0 && id != 1 {
//             let sig_share = sks.sign(msg);
//             let pks = pub_key_set.public_key_share(id);
//             assert!(pks.verify(&sig_share, msg));
//             sig_shares.insert(id, sig_share);
//         }
//     }
//
//     // Two signatures are over the threshold. They are enough to produce a signature that matches
//     // the public master key.
//     let sig = pub_key_set
//         .combine_signatures(&sig_shares)
//         .expect("The shares can be combined.");
//
//     assert!(pub_key_set.public_key().verify(&sig, msg));
//     */
// }

/// The algorithm is based on ideas from Distributed Key Generation in the Wild and A robust threshold elliptic curve digital signature providing a new verifiable secret sharing scheme.
///
/// In a trusted dealer scenario, the following steps occur:
///
/// Dealer generates a BivarPoly of degree t and publishes the BivarCommitment which is used to publicly verify the polynomial's values.
/// Dealer sends row m > 0 to node number m.
/// Node m, in turn, sends value number s to node number s.
/// This process continues until 2 t + 1 nodes confirm they have received a valid row. If there are at most t faulty nodes, we know that at least t + 1 correct nodes sent on an entry of every other node's column to that node.
/// This means every node can reconstruct its column, and the value at 0 of its column.
///
/// These values all lie on a univariate polynomial of degree t and can be used as secret keys.
/// In our dealerless environment, at least t + 1 nodes each generate a polynomial using the method above. The sum of the secret keys we received from each node is then used as our secret key. No single node knows the secret master key.
///

pub trait KeyGenerator {
    fn commit(&self);
}

// pub struct KeyGen {
//     //
// }

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        super::run();
    }
}
