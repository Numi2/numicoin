use numi_core::{block::Block, transaction::{Transaction, TransactionType}, crypto::Dilithium3Keypair, config::ConsensusConfig};

#[test]
fn debug_genesis_validation() {
    let kp = Dilithium3Keypair::new().unwrap();
    let consensus = ConsensusConfig::default();
    let mut reward_tx = Transaction::new(
        kp.public_key.clone(),
        TransactionType::MiningReward { block_height: 0, amount: consensus.initial_mining_reward },
        0,
    );
    reward_tx.sign(&kp).unwrap();
    let mut block = Block::new(0, [0u8;32], vec![reward_tx], 1, kp.public_key.clone());
    block.sign(&kp, None).unwrap();
    let res = block.validate(None, &consensus);
    println!("validation result: {:?}", res);
    assert!(res.is_ok());
}
