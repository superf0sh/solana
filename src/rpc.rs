//! The `rpc` module implements the Solana RPC interface.

use crate::cluster_info::ClusterInfo;
use crate::packet::PACKET_DATA_SIZE;
use crate::rpc_status::RpcSignatureStatus;
use crate::storage_stage::StorageState;
use bincode::{deserialize, serialize};
use bs58;
use jsonrpc_core::{Error, ErrorCode, Metadata, Result};
use jsonrpc_derive::rpc;
use solana_drone::drone::request_airdrop_transaction;
use solana_runtime::bank::{self, Bank, BankError};
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_sdk::transaction::Transaction;
use std::mem;
use std::net::{SocketAddr, UdpSocket};
use std::sync::{Arc, RwLock};
use std::thread::sleep;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct JsonRpcRequestProcessor {
    bank: Option<Arc<Bank>>,
    storage_state: StorageState,
}

impl JsonRpcRequestProcessor {
    fn bank(&self) -> Result<&Arc<Bank>> {
        self.bank.as_ref().ok_or(Error {
            code: ErrorCode::InternalError,
            message: "No bank available".into(),
            data: None,
        })
    }

    pub fn set_bank(&mut self, bank: &Arc<Bank>) {
        self.bank = Some(bank.clone());
    }

    pub fn new(storage_state: StorageState) -> Self {
        JsonRpcRequestProcessor {
            bank: None,
            storage_state,
        }
    }

    pub fn get_account_info(&self, pubkey: Pubkey) -> Result<Account> {
        self.bank()?
            .get_account(&pubkey)
            .ok_or_else(Error::invalid_request)
    }

    pub fn get_balance(&self, pubkey: Pubkey) -> Result<u64> {
        let val = self.bank()?.get_balance(&pubkey);
        Ok(val)
    }

    fn get_last_id(&self) -> Result<String> {
        let id = self.bank()?.last_id();
        Ok(bs58::encode(id).into_string())
    }

    pub fn get_signature_status(&self, signature: Signature) -> Option<bank::Result<()>> {
        self.bank()
            .ok()
            .and_then(|bank| bank.get_signature_status(&signature))
    }

    fn get_transaction_count(&self) -> Result<u64> {
        Ok(self.bank()?.transaction_count() as u64)
    }

    fn get_storage_mining_last_id(&self) -> Result<String> {
        let id = self.storage_state.get_last_id();
        Ok(bs58::encode(id).into_string())
    }

    fn get_storage_mining_entry_height(&self) -> Result<u64> {
        let entry_height = self.storage_state.get_entry_height();
        Ok(entry_height)
    }

    fn get_storage_pubkeys_for_entry_height(&self, entry_height: u64) -> Result<Vec<Pubkey>> {
        Ok(self
            .storage_state
            .get_pubkeys_for_entry_height(entry_height))
    }
}

fn get_leader_addr(cluster_info: &Arc<RwLock<ClusterInfo>>) -> Result<SocketAddr> {
    if let Some(leader_data) = cluster_info.read().unwrap().leader_data() {
        Ok(leader_data.tpu)
    } else {
        Err(Error {
            code: ErrorCode::InternalError,
            message: "No leader detected".into(),
            data: None,
        })
    }
}

fn verify_pubkey(input: String) -> Result<Pubkey> {
    let pubkey_vec = bs58::decode(input).into_vec().map_err(|err| {
        info!("verify_pubkey: invalid input: {:?}", err);
        Error::invalid_request()
    })?;
    if pubkey_vec.len() != mem::size_of::<Pubkey>() {
        info!(
            "verify_pubkey: invalid pubkey_vec length: {}",
            pubkey_vec.len()
        );
        Err(Error::invalid_request())
    } else {
        Ok(Pubkey::new(&pubkey_vec))
    }
}

fn verify_signature(input: &str) -> Result<Signature> {
    let signature_vec = bs58::decode(input).into_vec().map_err(|err| {
        info!("verify_signature: invalid input: {}: {:?}", input, err);
        Error::invalid_request()
    })?;
    if signature_vec.len() != mem::size_of::<Signature>() {
        info!(
            "verify_signature: invalid signature_vec length: {}",
            signature_vec.len()
        );
        Err(Error::invalid_request())
    } else {
        Ok(Signature::new(&signature_vec))
    }
}

#[derive(Clone)]
pub struct Meta {
    pub request_processor: Arc<RwLock<JsonRpcRequestProcessor>>,
    pub cluster_info: Arc<RwLock<ClusterInfo>>,
    pub rpc_addr: SocketAddr,
    pub drone_addr: SocketAddr,
}
impl Metadata for Meta {}

#[rpc]
pub trait RpcSol {
    type Metadata;

    #[rpc(meta, name = "confirmTransaction")]
    fn confirm_transaction(&self, _: Self::Metadata, _: String) -> Result<bool>;

    #[rpc(meta, name = "getAccountInfo")]
    fn get_account_info(&self, _: Self::Metadata, _: String) -> Result<Account>;

    #[rpc(meta, name = "getBalance")]
    fn get_balance(&self, _: Self::Metadata, _: String) -> Result<u64>;

    #[rpc(meta, name = "getLastId")]
    fn get_last_id(&self, _: Self::Metadata) -> Result<String>;

    #[rpc(meta, name = "getSignatureStatus")]
    fn get_signature_status(&self, _: Self::Metadata, _: String) -> Result<RpcSignatureStatus>;

    #[rpc(meta, name = "getTransactionCount")]
    fn get_transaction_count(&self, _: Self::Metadata) -> Result<u64>;

    #[rpc(meta, name = "requestAirdrop")]
    fn request_airdrop(&self, _: Self::Metadata, _: String, _: u64) -> Result<String>;

    #[rpc(meta, name = "sendTransaction")]
    fn send_transaction(&self, _: Self::Metadata, _: Vec<u8>) -> Result<String>;

    #[rpc(meta, name = "getStorageMiningLastId")]
    fn get_storage_mining_last_id(&self, _: Self::Metadata) -> Result<String>;

    #[rpc(meta, name = "getStorageMiningEntryHeight")]
    fn get_storage_mining_entry_height(&self, _: Self::Metadata) -> Result<u64>;

    #[rpc(meta, name = "getStoragePubkeysForEntryHeight")]
    fn get_storage_pubkeys_for_entry_height(
        &self,
        _: Self::Metadata,
        _: u64,
    ) -> Result<Vec<Pubkey>>;
}

pub struct RpcSolImpl;
impl RpcSol for RpcSolImpl {
    type Metadata = Meta;

    fn confirm_transaction(&self, meta: Self::Metadata, id: String) -> Result<bool> {
        info!("confirm_transaction rpc request received: {:?}", id);
        self.get_signature_status(meta, id)
            .map(|status| status == RpcSignatureStatus::Confirmed)
    }

    fn get_account_info(&self, meta: Self::Metadata, id: String) -> Result<Account> {
        info!("get_account_info rpc request received: {:?}", id);
        let pubkey = verify_pubkey(id)?;
        meta.request_processor
            .read()
            .unwrap()
            .get_account_info(pubkey)
    }

    fn get_balance(&self, meta: Self::Metadata, id: String) -> Result<u64> {
        info!("get_balance rpc request received: {:?}", id);
        let pubkey = verify_pubkey(id)?;
        meta.request_processor.read().unwrap().get_balance(pubkey)
    }

    fn get_last_id(&self, meta: Self::Metadata) -> Result<String> {
        info!("get_last_id rpc request received");
        meta.request_processor.read().unwrap().get_last_id()
    }

    fn get_signature_status(&self, meta: Self::Metadata, id: String) -> Result<RpcSignatureStatus> {
        info!("get_signature_status rpc request received: {:?}", id);
        let signature = verify_signature(&id)?;
        let res = meta
            .request_processor
            .read()
            .unwrap()
            .get_signature_status(signature);

        let status = {
            if res.is_none() {
                RpcSignatureStatus::SignatureNotFound
            } else {
                match res.unwrap() {
                    Ok(_) => RpcSignatureStatus::Confirmed,
                    Err(BankError::AccountInUse) => RpcSignatureStatus::AccountInUse,
                    Err(BankError::AccountLoadedTwice) => RpcSignatureStatus::AccountLoadedTwice,
                    Err(BankError::ProgramError(_, _)) => RpcSignatureStatus::ProgramRuntimeError,
                    Err(err) => {
                        trace!("mapping {:?} to GenericFailure", err);
                        RpcSignatureStatus::GenericFailure
                    }
                }
            }
        };
        info!("get_signature_status rpc request status: {:?}", status);
        Ok(status)
    }

    fn get_transaction_count(&self, meta: Self::Metadata) -> Result<u64> {
        info!("get_transaction_count rpc request received");
        meta.request_processor
            .read()
            .unwrap()
            .get_transaction_count()
    }

    fn request_airdrop(&self, meta: Self::Metadata, id: String, tokens: u64) -> Result<String> {
        trace!("request_airdrop id={} tokens={}", id, tokens);
        let pubkey = verify_pubkey(id)?;

        let last_id = meta.request_processor.read().unwrap().bank()?.last_id();
        let transaction = request_airdrop_transaction(&meta.drone_addr, &pubkey, tokens, last_id)
            .map_err(|err| {
            info!("request_airdrop_transaction failed: {:?}", err);
            Error::internal_error()
        })?;;

        let data = serialize(&transaction).map_err(|err| {
            info!("request_airdrop: serialize error: {:?}", err);
            Error::internal_error()
        })?;

        let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let transactions_addr = get_leader_addr(&meta.cluster_info)?;
        transactions_socket
            .send_to(&data, transactions_addr)
            .map_err(|err| {
                info!("request_airdrop: send_to error: {:?}", err);
                Error::internal_error()
            })?;

        let signature = transaction.signatures[0];
        let now = Instant::now();
        let mut signature_status;
        loop {
            signature_status = meta
                .request_processor
                .read()
                .unwrap()
                .get_signature_status(signature);

            if signature_status == Some(Ok(())) {
                info!("airdrop signature ok");
                return Ok(bs58::encode(signature).into_string());
            } else if now.elapsed().as_secs() > 5 {
                info!("airdrop signature timeout");
                return Err(Error::internal_error());
            }
            sleep(Duration::from_millis(100));
        }
    }

    fn send_transaction(&self, meta: Self::Metadata, data: Vec<u8>) -> Result<String> {
        let tx: Transaction = deserialize(&data).map_err(|err| {
            info!("send_transaction: deserialize error: {:?}", err);
            Error::invalid_request()
        })?;
        if data.len() >= PACKET_DATA_SIZE {
            info!(
                "send_transaction: transaction too large: {} bytes (max: {} bytes)",
                data.len(),
                PACKET_DATA_SIZE
            );
            return Err(Error::invalid_request());
        }
        let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let transactions_addr = get_leader_addr(&meta.cluster_info)?;
        trace!("send_transaction: leader is {:?}", &transactions_addr);
        transactions_socket
            .send_to(&data, transactions_addr)
            .map_err(|err| {
                info!("send_transaction: send_to error: {:?}", err);
                Error::internal_error()
            })?;
        let signature = bs58::encode(tx.signatures[0]).into_string();
        trace!(
            "send_transaction: sent {} bytes, signature={}",
            data.len(),
            signature
        );
        Ok(signature)
    }

    fn get_storage_mining_last_id(&self, meta: Self::Metadata) -> Result<String> {
        meta.request_processor
            .read()
            .unwrap()
            .get_storage_mining_last_id()
    }

    fn get_storage_mining_entry_height(&self, meta: Self::Metadata) -> Result<u64> {
        meta.request_processor
            .read()
            .unwrap()
            .get_storage_mining_entry_height()
    }

    fn get_storage_pubkeys_for_entry_height(
        &self,
        meta: Self::Metadata,
        entry_height: u64,
    ) -> Result<Vec<Pubkey>> {
        meta.request_processor
            .read()
            .unwrap()
            .get_storage_pubkeys_for_entry_height(entry_height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster_info::NodeInfo;
    use jsonrpc_core::{MetaIoHandler, Response};
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::hash::{hash, Hash};
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction::SystemTransaction;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::thread;

    fn start_rpc_handler_with_tx(pubkey: Pubkey) -> (MetaIoHandler<Meta>, Meta, Hash, Keypair) {
        let (genesis_block, alice) = GenesisBlock::new(10_000);
        let bank = Arc::new(Bank::new(&genesis_block));

        let last_id = bank.last_id();
        let tx = SystemTransaction::new_move(&alice, pubkey, 20, last_id, 0);
        bank.process_transaction(&tx).expect("process transaction");

        let request_processor = Arc::new(RwLock::new(JsonRpcRequestProcessor::new(
            StorageState::default(),
        )));
        request_processor.write().unwrap().set_bank(&bank);
        let cluster_info = Arc::new(RwLock::new(ClusterInfo::new(NodeInfo::default())));
        let leader = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.1:1234"));

        cluster_info.write().unwrap().insert_info(leader.clone());
        cluster_info.write().unwrap().set_leader(leader.id);
        let rpc_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);
        let drone_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);

        let mut io = MetaIoHandler::default();
        let rpc = RpcSolImpl;
        io.extend_with(rpc.to_delegate());
        let meta = Meta {
            request_processor,
            cluster_info,
            drone_addr,
            rpc_addr,
        };
        (io, meta, last_id, alice)
    }

    #[test]
    fn test_rpc_request_processor_new() {
        let (genesis_block, alice) = GenesisBlock::new(10_000);
        let bob_pubkey = Keypair::new().pubkey();
        let bank = Arc::new(Bank::new(&genesis_block));
        let mut request_processor = JsonRpcRequestProcessor::new(StorageState::default());
        request_processor.set_bank(&bank);
        thread::spawn(move || {
            let last_id = bank.last_id();
            let tx = SystemTransaction::new_move(&alice, bob_pubkey, 20, last_id, 0);
            bank.process_transaction(&tx).expect("process transaction");
        })
        .join()
        .unwrap();
        assert_eq!(request_processor.get_transaction_count().unwrap(), 1);
    }

    #[test]
    fn test_rpc_get_balance() {
        let bob_pubkey = Keypair::new().pubkey();
        let (io, meta, _last_id, _alice) = start_rpc_handler_with_tx(bob_pubkey);

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getBalance","params":["{}"]}}"#,
            bob_pubkey
        );
        let res = io.handle_request_sync(&req, meta);
        let expected = format!(r#"{{"jsonrpc":"2.0","result":20,"id":1}}"#);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_get_tx_count() {
        let bob_pubkey = Keypair::new().pubkey();
        let (io, meta, _last_id, _alice) = start_rpc_handler_with_tx(bob_pubkey);

        let req = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getTransactionCount"}}"#);
        let res = io.handle_request_sync(&req, meta);
        let expected = format!(r#"{{"jsonrpc":"2.0","result":1,"id":1}}"#);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_get_account_info() {
        let bob_pubkey = Keypair::new().pubkey();
        let (io, meta, _last_id, _alice) = start_rpc_handler_with_tx(bob_pubkey);

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getAccountInfo","params":["{}"]}}"#,
            bob_pubkey
        );
        let res = io.handle_request_sync(&req, meta);
        let expected = r#"{
            "jsonrpc":"2.0",
            "result":{
                "owner": [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
                "tokens": 20,
                "userdata": [],
                "executable": false
            },
            "id":1}
        "#;
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_confirm_tx() {
        let bob_pubkey = Keypair::new().pubkey();
        let (io, meta, last_id, alice) = start_rpc_handler_with_tx(bob_pubkey);
        let tx = SystemTransaction::new_move(&alice, bob_pubkey, 20, last_id, 0);

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"confirmTransaction","params":["{}"]}}"#,
            tx.signatures[0]
        );
        let res = io.handle_request_sync(&req, meta);
        let expected = format!(r#"{{"jsonrpc":"2.0","result":true,"id":1}}"#);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_get_signature_status() {
        let bob_pubkey = Keypair::new().pubkey();
        let (io, meta, last_id, alice) = start_rpc_handler_with_tx(bob_pubkey);
        let tx = SystemTransaction::new_move(&alice, bob_pubkey, 20, last_id, 0);

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getSignatureStatus","params":["{}"]}}"#,
            tx.signatures[0]
        );
        let res = io.handle_request_sync(&req, meta.clone());
        let expected = format!(r#"{{"jsonrpc":"2.0","result":"Confirmed","id":1}}"#);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);

        // Test getSignatureStatus request on unprocessed tx
        let tx = SystemTransaction::new_move(&alice, bob_pubkey, 10, last_id, 0);
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getSignatureStatus","params":["{}"]}}"#,
            tx.signatures[0]
        );
        let res = io.handle_request_sync(&req, meta);
        let expected = format!(r#"{{"jsonrpc":"2.0","result":"SignatureNotFound","id":1}}"#);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_get_last_id() {
        let bob_pubkey = Keypair::new().pubkey();
        let (io, meta, last_id, _alice) = start_rpc_handler_with_tx(bob_pubkey);

        let req = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getLastId"}}"#);
        let res = io.handle_request_sync(&req, meta);
        let expected = format!(r#"{{"jsonrpc":"2.0","result":"{}","id":1}}"#, last_id);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_fail_request_airdrop() {
        let bob_pubkey = Keypair::new().pubkey();
        let (io, meta, _last_id, _alice) = start_rpc_handler_with_tx(bob_pubkey);

        // Expect internal error because no leader is running
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"requestAirdrop","params":["{}", 50]}}"#,
            bob_pubkey
        );
        let res = io.handle_request_sync(&req, meta);
        let expected =
            r#"{"jsonrpc":"2.0","error":{"code":-32603,"message":"Internal error"},"id":1}"#;
        let expected: Response =
            serde_json::from_str(expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_send_bad_tx() {
        let (genesis_block, _) = GenesisBlock::new(10_000);
        let bank = Arc::new(Bank::new(&genesis_block));

        let mut io = MetaIoHandler::default();
        let rpc = RpcSolImpl;
        io.extend_with(rpc.to_delegate());
        let meta = Meta {
            request_processor: {
                let mut request_processor = JsonRpcRequestProcessor::new(StorageState::default());
                request_processor.set_bank(&bank);
                Arc::new(RwLock::new(request_processor))
            },
            cluster_info: Arc::new(RwLock::new(ClusterInfo::new(NodeInfo::default()))),
            drone_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
            rpc_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
        };

        let req =
            r#"{"jsonrpc":"2.0","id":1,"method":"sendTransaction","params":[[0,0,0,0,0,0,0,0]]}"#;
        let res = io.handle_request_sync(req, meta.clone());
        let expected =
            r#"{"jsonrpc":"2.0","error":{"code":-32600,"message":"Invalid request"},"id":1}"#;
        let expected: Response =
            serde_json::from_str(expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_get_leader_addr() {
        let cluster_info = Arc::new(RwLock::new(ClusterInfo::new(NodeInfo::default())));
        assert_eq!(
            get_leader_addr(&cluster_info),
            Err(Error {
                code: ErrorCode::InternalError,
                message: "No leader detected".into(),
                data: None,
            })
        );
        let leader = NodeInfo::new_with_socketaddr(&socketaddr!("127.0.0.1:1234"));
        cluster_info.write().unwrap().insert_info(leader.clone());
        cluster_info.write().unwrap().set_leader(leader.id);
        assert_eq!(
            get_leader_addr(&cluster_info),
            Ok(socketaddr!("127.0.0.1:1234"))
        );
    }

    #[test]
    fn test_rpc_verify_pubkey() {
        let pubkey = Keypair::new().pubkey();
        assert_eq!(verify_pubkey(pubkey.to_string()).unwrap(), pubkey);
        let bad_pubkey = "a1b2c3d4";
        assert_eq!(
            verify_pubkey(bad_pubkey.to_string()),
            Err(Error::invalid_request())
        );
    }

    #[test]
    fn test_rpc_verify_signature() {
        let tx = SystemTransaction::new_move(
            &Keypair::new(),
            Keypair::new().pubkey(),
            20,
            hash(&[0]),
            0,
        );
        assert_eq!(
            verify_signature(&tx.signatures[0].to_string()).unwrap(),
            tx.signatures[0]
        );
        let bad_signature = "a1b2c3d4";
        assert_eq!(
            verify_signature(&bad_signature.to_string()),
            Err(Error::invalid_request())
        );
    }
}
