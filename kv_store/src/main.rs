use crate::kv::KVCommand;
use crate::server::Server;
use omnipaxos::*;
use omnipaxos_storage::persistent_storage::{PersistentStorage, PersistentStorageConfig};
use std::env;
use std::sync::Arc;
use std::fs;
use tokio::sync::Mutex;
use tokio;

#[macro_use]
extern crate lazy_static;

mod database;
mod kv;
mod network;
mod server;

lazy_static! {
    pub static ref NODES: Vec<u64> = {
        let var = env::var("NODES").expect("Missing NODES environment variable");
        serde_json::from_str::<Vec<u64>>(&var).expect("Invalid NODES format")
    };

    pub static ref PID: u64 = {
        let var = env::var("PID").expect("Missing PID environment variable");
        let x = var.parse().expect("PIDs must be u64");
        if x == 0 {
            panic!("PIDs cannot be 0");
        }
        x
    };
    pub static ref CONFIG_ID: u32 = {
        let var = env::var("CONFIG_ID").expect("Missing Config_Id environment variable");
        let x = var.parse().expect("Config IDs must be u64");
        if x == 0 {
            panic!("Configs cannot be 0");
        }
        x
    };
}

// ✅ Define PersistentStorage type
type OmniPaxosKV = OmniPaxos<KVCommand, PersistentStorage<KVCommand>>;

#[tokio::main]
async fn main() {
    println!(" Starting server with PID {}", *PID);

    let server_config = ServerConfig {
        pid: *PID,
        election_tick_timeout: 5,
        ..Default::default()
    };

    let cluster_config = ClusterConfig {
        configuration_id: (*CONFIG_ID).clone(),
        nodes: (*NODES).clone(),
        ..Default::default()
    };

    let op_config = OmniPaxosConfig {
        server_config,
        cluster_config,
    };

    // ✅ Prevent RocksDB Lock Errors
    let storage_path = format!("/data/omnipaxos_storage_{}", *PID);
    let backup_path = format!("/data/omnipaxos_storage_backup_{}", *PID);
    let db_path = "/data/db";

    fn remove_lock_file(path: &str) {
        let lock_file = format!("{}/LOCK", path);
        if std::path::Path::new(&lock_file).exists() {
            println!("🛠 Removing stale lock file: {}", lock_file);
            fs::remove_file(&lock_file).expect("Failed to remove lock file");
        }
    }

    remove_lock_file(&storage_path);
    remove_lock_file(&db_path);


    let mut persistent_storage;
loop {
    persistent_storage = PersistentStorage::open(PersistentStorageConfig::with_path(storage_path.clone()));
    if let PersistentStorage { .. } = persistent_storage {
        println!("✅ PersistentStorage initialized successfully.");
        break;
    }
    println!("⚠️ WARNING: PersistentStorage failed to open. Retrying in 5 seconds...");
    std::thread::sleep(std::time::Duration::from_secs(5));
}

    // // ✅ Manually check if PersistentStorage fails without using `Err` or `Ok`
    // let persistent_storage_primary = PersistentStorage::open(PersistentStorageConfig::with_path(storage_path.clone()));

    //  persistent_storage = if let PersistentStorage { .. } = persistent_storage_primary {
    //     println!("✅ Primary PersistentStorage opened successfully.");
    //     persistent_storage_primary
    // } else {
    //     println!("⚠️ WARNING: Primary storage failed...");
       
    //         panic!(" CRITICAL: Failed to open both primary PersistentStorage!");
        
    // };

    println!("✅ PersistentStorage initialized successfully.");

    //  Initialize OmniPaxos and Ensure Cluster Recovery
    let omni_paxos_result = op_config.clone().build(persistent_storage);

    if let Ok(omni_paxos) = omni_paxos_result {
        //  Use Arc<Mutex<T>> to allow multiple async tasks to access `server`
        let server = Arc::new(Mutex::new(Server::new(omni_paxos, db_path).await));

        
         //  Start the server
        server.lock().await.run().await;
    } else {
        if let Err(e) = omni_paxos_result {
            println!("Failed to initialize OmniPaxos: {:?}", e);
	    
        }
    }
}
