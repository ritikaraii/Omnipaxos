use crate::database::Database;
use crate::kv::KVCommand;
use crate::{
    network::{Message, Network},
    OmniPaxosKV,
    NODES,
    PID as MY_PID,
    CONFIG_ID
};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::{env, fs};
use tokio::sync::Mutex;
use omnipaxos_storage::persistent_storage::{PersistentStorage, PersistentStorageConfig};
use omnipaxos::{ClusterConfig, ServerConfig};
use omnipaxos::storage::StopSign;
use omnipaxos::util::LogEntry;
use serde::Deserialize;
use serde::Serialize;

use std::time::{Duration, Instant};
use tokio::time;


#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum APIResponse {
    Decided(u64),
    Get(String, Option<String>),
}

pub struct Server {
    pub omni_paxos: OmniPaxosKV,
    pub network: Network,
    pub database: Database,
    pub last_decided_idx: u64,
    pub current_heartbeats: HashMap<u64, Instant>,
    pub expired_nodes: HashSet<u64>,
    pub running: Arc<AtomicBool>,
   
    //pub trigger: Trigger,
    
}

impl Server {
    pub async fn new(omni_paxos: OmniPaxosKV, db_path: &str) -> Self {
        Server {
            omni_paxos,
            network: Network::new().await,
            database: Database::new(db_path),
            last_decided_idx: 0,
            current_heartbeats: HashMap::new(),
            expired_nodes: HashSet::new(),
            running: Arc::new(AtomicBool::new(true))
            //trigger: Trigger::new().await,
       
            
        }
    }
    async fn process_incoming_msgs(&mut self) {
        let messages = self.network.get_received().await;
        for msg in messages {
            match msg {
                Message::APIRequest(kv_cmd) => match kv_cmd {
                    KVCommand::Get(key) => {
                        let value = self.database.handle_command(KVCommand::Get(key.clone()));
                        let reply = Message::APIResponse(APIResponse::Get(key, value));
                        self.network.send(0, reply).await;
                    }
                    KVCommand::Reconfigure(key) => {
                        println!("Manual reconfigure command received: {}", key);
                        let mut all_nodes = NODES.clone();
                        if let Ok(new_node) = key.parse::<u64>() {
                            if !all_nodes.contains(&new_node) {
                                all_nodes.push(new_node);
                                println!("Added node {} to NODES cluster: {:?}", new_node, all_nodes);
                            } else {
                                println!("Node {} already exists in NODES cluster", new_node);
                            }
                        } else {
                            println!("Invalid node ID: {}", key);
                            return;
                        }
                        // from patch command sent from client, we increase the config for new configuration
                        let new_cluster = ClusterConfig {
                            configuration_id: *CONFIG_ID + 1,
                            nodes: all_nodes,
                            ..Default::default()
                        };
                        let metadata = None;
                        //this will send stopsign from omnipaxos library...
                        self.omni_paxos.reconfigure(new_cluster, metadata)
                        .expect("Failed to propose reconfiguration for new pod and increased cluster id");
                    }
                    cmd => {
                        println!(" Received PUT/DELETE command on Node {}: {:?}", *MY_PID, cmd);
                        // Appending the command to check if it fails or is successful
                        if let Err(e) = self.omni_paxos.append(cmd) {
                            println!(" Failed to append command on Node {}: {:?}", *MY_PID, e);
                        } else {
                            println!("Successfully appended command on Node {}.", *MY_PID);
                        }
                    }
                },
                Message::OmniPaxosMsg(msg) => {
                    let sender = msg.get_sender();
                    self.current_heartbeats.insert(sender, Instant::now());
                    self.expired_nodes.remove(&sender);
                    self.omni_paxos.handle_incoming(msg);
                }
                _ =>  {
                    println!("Received unimplemented msg {:?}", msg);
                },
            }
        }
    }


    async fn send_outgoing_msgs(&mut self) {
        let messages = self.omni_paxos.outgoing_messages();
        for msg in messages {
            let receiver = msg.get_receiver();
            self.network
                .send(receiver, Message::OmniPaxosMsg(msg))
                .await;
        }
    }

    // async fn request_log_sync_from_leader(&mut self) {
    //     if let Some((leader_id, _)) = self.omni_paxos.get_current_leader() {
    //         if leader_id != *MY_PID {
    //             println!("Requesting log sync from leader node: {}", leader_id);
                
    //             // Explicitly request missing logs
    //             self.omni_paxos.reconnected(leader_id);
    
    //             // Ensure reconnected state
    //             self.omni_paxos.reconnected(*MY_PID);
    //         } else {
    //             println!("Node {} is the leader, ensuring log consistency", *MY_PID);
    //             if let Err(e) = self.omni_paxos.trim(Some(self.last_decided_idx as usize)) {
    //                 println!("Failed to trim logs: {:?}", e);
    //             }
    //         }
    //     } else {
    //         println!("No leader detected. Retrying in 5 seconds...");
    //         tokio::time::sleep(Duration::from_secs(5)).await;
    //         self.omni_paxos.reconnected(*MY_PID);
    //     }
    // }
    
    pub async fn request_log_sync_from_leader(&mut self) {
        if let Some((leader_id, _)) = self.omni_paxos.get_current_leader() {
            if leader_id != *MY_PID {
                println!(" Requesting log sync from leader node: {}", leader_id);
                self.omni_paxos.reconnected(leader_id);
                self.omni_paxos.reconnected(*MY_PID);
    
                let mut retries = 10; // Retry up to 10 times
                while retries > 0 {
                    tokio::time::sleep(Duration::from_millis(100)).await; // Shorter wait
    
                    let new_decided_idx = self.omni_paxos.get_decided_idx();
                    if new_decided_idx > self.last_decided_idx as usize {
                        println!(
                            " Logs successfully synced! New index: {} (previous: {})",
                            new_decided_idx, self.last_decided_idx
                        );
                        self.last_decided_idx = new_decided_idx as u64;
                        return;
                    }
    
                    retries -= 1;
                }
                println!("⚠️ Log sync timed out after multiple attempts.");
            } else {
                println!("⚡ Node {} is the leader, ensuring log consistency...", *MY_PID);
                if let Err(e) = self.omni_paxos.trim(Some(self.last_decided_idx as usize)) {
                    println!("⚠️ Failed to trim logs: {:?}", e);
                }
            }
        } else {
            println!("⚠️ No leader detected. Retrying in 5 seconds...");
            tokio::time::sleep(Duration::from_secs(5)).await;
            self.omni_paxos.reconnected(*MY_PID);
        }
    }
    
    

    async fn handle_decided_entries(&mut self) {
        let new_decided_idx = self.omni_paxos.get_decided_idx();
    
        if self.last_decided_idx < new_decided_idx as u64 {
            println!(
                "🔹 New decided index: {} (last_decided_idx: {})",
                new_decided_idx, self.last_decided_idx
            );
    
            if let Some(decided_entries) = self.omni_paxos.read_decided_suffix(self.last_decided_idx as usize) {
                println!("📝 Applying missing decided entries: {:?}", decided_entries);
                self.update_database(decided_entries);
            } else {
                println!("❌ No decided entries found. Attempting log recovery...");
    
                if let Some(all_entries) = self.omni_paxos.read_entries(0..self.last_decided_idx as usize) {
                    println!("🔄 Recovering logs from storage...");
                    self.update_database(all_entries);
                } else {
                    println!("🚨 No logs found. Requesting sync from leader...");
                    self.request_log_sync_from_leader().await;
                }
            }
    
            self.last_decided_idx = new_decided_idx as u64;
            let msg = Message::APIResponse(APIResponse::Decided(new_decided_idx as u64));
            self.network.send(0, msg).await;
        }
    
        if new_decided_idx % 5 == 0 {
            match self.omni_paxos.read_decided_suffix(0) {
                Some(log) => println!("📝 Log before snapshot: {:?}", log),
                None => println!("⚠️ No logs found before snapshot."),
            }
            self.omni_paxos
                .snapshot(Some(new_decided_idx), true)
                .expect("❌ Failed to snapshot");
    
            match self.omni_paxos.read_decided_suffix(0) {
                Some(log) => println!("✅ Log after snapshot: {:?}", log),
                None => println!("⚠️ No logs found after snapshot."),
            }
        }
    }
    
    
    
    
    fn update_database(&mut self, decided_entries: Vec<LogEntry<KVCommand>>) {
        for entry in decided_entries {
            match entry {
                LogEntry::Decided(cmd) => {
                    self.database.handle_command(cmd);
                }
                LogEntry::StopSign(stopsign, boolval) => {
                   println!(
                        "StopSign received: new config id: {:?}, flag: {}. My config id: {:?}",
                        stopsign.next_config.configuration_id, boolval, *CONFIG_ID
                    );                    
		    self.handle_stop_sign(stopsign, boolval);
                    println!("Handled stopsign with new configuration.");            
                }
                _ => {}
            }
        }
    }

    pub fn halt(&self) {
        self.running.store(false, std::sync::atomic::Ordering::Relaxed);
    }
    
    pub fn resume(&self) {
        self.running.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    fn handle_stop_sign(&mut self, stopsign: StopSign, flag: bool) {
        if stopsign.next_config.configuration_id <= *CONFIG_ID {
            println!("Ignoring outdated StopSign. Current: {}, Received: {}", *CONFIG_ID, stopsign.next_config.configuration_id);
            return;
        }
        // Check if the new configuration is more recent and this node is in it.
        if stopsign.next_config.configuration_id > *CONFIG_ID
	            && stopsign.next_config.nodes.contains(&MY_PID) {

            println!("Initiating configuration transition...");
            self.halt(); // Stop accepting new commands on the old configuration.
            
            let cur_idx = self.omni_paxos.get_decided_idx();
            println!("Snapshot pre-transition: {:?}", self.omni_paxos.read_decided_suffix(0).unwrap());
            self.omni_paxos
                .snapshot(Some(cur_idx), true)
                .expect("Snapshot failed");
            println!("Snapshot post-transition: {:?}", self.omni_paxos.read_decided_suffix(0).unwrap());

            // Build new persistent storage paths for the new configuration
            let new_storage = format!("/data/omnipaxos_storage_{}_{}", *MY_PID, stopsign.next_config.configuration_id);
            let new_db = format!("data/db{}", stopsign.next_config.configuration_id);

            // Remove any stale lock files
            fn clear_lock(path: &str) {
                let lock_file = format!("{}/LOCK", path);
                if std::path::Path::new(&lock_file).exists() {
                    println!("Clearing lock file: {}", lock_file);
                    fs::remove_file(&lock_file).expect("Failed to remove lock file");
                }
            }
            clear_lock(&new_storage);
            clear_lock(&new_db);

            // Reinitialize persistent storage with the new configuration.
            let persistent_storage = PersistentStorage::new(PersistentStorageConfig::with_path(new_storage.clone()));
           
            let current_config = ServerConfig { pid: *MY_PID, ..Default::default() };
            let new_instance = stopsign.next_config.build_for_server(current_config, persistent_storage);

            if let Ok(new_omni) = new_instance {
                println!("Reconfiguration, after addition of new pod ");
                self.omni_paxos = new_omni;
                self.database = Database::new(&new_db);
                self.last_decided_idx = 0;
                self.current_heartbeats.clear();   
                self.expired_nodes.clear();
                println!("Requesting log sync after reconfiguration...");
                //self.omni_paxos.reconnected(*MY_PID);
                self.resume();
                println!("restarting process to load new config...");
                std::process::exit(1);
            }
        }
    }

   

    pub(crate) async fn run(&mut self) {
        let mut msg_interval = time::interval(Duration::from_millis(1));
        let mut tick_interval = time::interval(Duration::from_millis(10));

         // 🚀 Ensure all nodes sync on startup
        self.request_log_sync_from_leader().await;

        while self.running.load(std::sync::atomic::Ordering::Relaxed) {
            tokio::select! {
                biased;
                _ = msg_interval.tick() => {
                    self.process_incoming_msgs().await;
                    self.send_outgoing_msgs().await;
                    self.handle_decided_entries().await;
                },
   
                _ = tick_interval.tick() => {
                    self.omni_paxos.tick();
		        
                    let now = Instant::now();
                    let expired_nodes: Vec<u64> = self.current_heartbeats
                        .iter()
                        .filter(|(_, last_seen)| now.duration_since(**last_seen) >= Duration::from_millis(100))
                        .map(|(sender_id, _)| *sender_id)
                        .collect();

                    // Mark nodes as expired if they aren't already
                    for sender_id in &expired_nodes {
                        if !self.expired_nodes.contains(sender_id) {
                            println!("Node {} is unresponsive. Marking for reconnection...", sender_id);
                            self.expired_nodes.insert(*sender_id);
                        }
                    }

                    // Keep retrying reconnection for expired nodes
                    for sender_id in &self.expired_nodes {
                        println!("Trying to reconnect to node {}...", sender_id);
                        self.omni_paxos.reconnected(*sender_id);
                    }

                    // Remove expired heartbeats from tracking
                    for sender_id in expired_nodes {
                        self.current_heartbeats.remove(&sender_id);
                    }
                        
                    if self.omni_paxos.get_current_leader().is_none() {
                        println!(" No leader detected! Ensuring logs are synced before reconnecting node {}...", *MY_PID);
    
                        if self.omni_paxos.read_decided_suffix(0).is_none() {
                            if self.omni_paxos.get_current_leader().map(|(leader_id, _)| leader_id) == Some(*MY_PID) {
                                if let Err(e) = self.omni_paxos.trim(Some(self.last_decided_idx as usize)) {
                                    println!(" Log sync failed: {:?}. Skipping trim...", e);
                                }
                            } 
                        }
                        println!(" Reconnecting node {}...", *MY_PID);
                        self.omni_paxos.reconnected(*MY_PID);
                    }
                    
                },
                else => (),
            }
        }
    }
}


