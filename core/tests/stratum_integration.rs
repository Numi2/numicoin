use std::net::TcpListener;
use std::sync::Arc;
use tokio::net::TcpStream;
use numi_core::RwLock;
use tempfile::tempdir;

use numi_core::config::Config;
use numi_core::mining_service::MiningService;
use numi_core::miner::Miner;
use numi_core::blockchain::NumiBlockchain;
use numi_core::storage::BlockchainStorage;
use numi_core::network::NetworkManager;
use numi_core::stratum_server::StratumV2Server;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_stratum_server_accepts_connection() {
    // Allocate an ephemeral port by binding and immediately dropping
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    // Prepare supporting services
    let chain = Arc::new(RwLock::new(NumiBlockchain::new(numi_core::config::ConsensusConfig::default()).unwrap()));
    let storage_dir = tempdir().unwrap();
    let _storage = Arc::new(BlockchainStorage::new(storage_dir.path()).unwrap());

    // Prepare NetworkManager using current constructor
    let network_cfg = numi_core::config::NetworkConfig::default();
    let (in_tx, _in_rx) = futures::channel::mpsc::unbounded();
    let (_network_mgr, network_handle) = NetworkManager::new(&network_cfg, in_tx).unwrap();
    let cfg_default = Config::default();
    let miner = Arc::new(RwLock::new(Miner::new(&cfg_default).unwrap()));

    // Configure Stratum bind address and port
    let mut cfg = Config::default();
    cfg.mining.enabled = true;
    cfg.mining.stratum_bind_address = "127.0.0.1".to_string();
    cfg.mining.stratum_bind_port = port;

    // Build the mining service
    let service = Arc::new(
        MiningService::new(
            chain,
            network_handle,
            miner,
            cfg.mining.clone(),
            cfg.consensus.clone(),
        )
    );

    // Start the Stratum server in the background
    tokio::spawn(async move {
        let stratum_server = StratumV2Server::new(service);
        stratum_server.start().await.unwrap();
    });

    // Allow server some time to bind
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Attempt a TCP connection
    let stream = TcpStream::connect(("127.0.0.1", port)).await;
    assert!(stream.is_ok(), "Stratum server did not accept TCP connections");
}