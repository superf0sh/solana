use solana::rpc_request::RpcClient;
use solana::thin_client::new_fullnode;
use solana_drone::drone::run_local_drone;
use solana_sdk::signature::KeypairUtil;
use solana_wallet::wallet::{process_command, WalletCommand, WalletConfig};
use std::fs::remove_dir_all;
use std::sync::mpsc::channel;

#[test]
fn test_wallet_request_airdrop() {
    let (server, leader_data, alice, ledger_path) = new_fullnode();
    let server_exit = server.run(None);
    let (sender, receiver) = channel();
    run_local_drone(alice, sender);
    let drone_addr = receiver.recv().unwrap();

    let mut bob_config = WalletConfig::default();
    bob_config.drone_port = drone_addr.port();
    bob_config.rpc_port = leader_data.rpc.port();
    bob_config.command = WalletCommand::Airdrop(50);

    let sig_response = process_command(&bob_config);
    sig_response.unwrap();

    let rpc_client = RpcClient::new_from_socket(leader_data.rpc);

    let balance = rpc_client
        .retry_get_balance(1, bob_config.id.pubkey(), 1)
        .unwrap()
        .unwrap();
    assert_eq!(balance, 50);

    server_exit();
    remove_dir_all(ledger_path).unwrap();
}
