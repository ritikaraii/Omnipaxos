use crate::database::Database;
use crate::kv::KVCommand;
use crate::{
    network::{Message, Network},
    OmniPaxosKV,
    NODES,
    PID as MY_PID,
    CONFIG_ID
};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread::current;
use std::{env, fs};
use tokio::sync::Mutex;
use omnipaxos::messages::ballot_leader_election::*;
use omnipaxos_storage::persistent_storage::{PersistentStorage, PersistentStorageConfig};
use omnipaxos::{ClusterConfig, ServerConfig};
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
    pub heartbeats: HashMap<u64, Instant>,
    pub expired_nodes: HashSet<u64>,
    pub running: Arc<AtomicBool>,
    
}

impl Server {
    pub async fn new(omni_paxos: OmniPaxosKV, db_path: &str) -> Self {
        Self {
            omni_paxos,
            network: Network::new().await,
            database: Database::new(db_path),
            last_decided_idx: 0,
            heartbeats: HashMap::new(),
            expired_nodes: HashSet::new(),
            running: Arc::new(AtomicBool::new(true))
            
        }
    }
    async fn process_incoming_msgs(&mut self) {
        let messages = self.network.get_received().await;
        for msg in messages {
            match msg {
                Message::APIRequest(kv_cmd) => match kv_cmd {
                    KVCommand::Get(key) => {
                        let value = self.database.handle_command(KVCommand::Get(key.clone()));
                        let msg = Message::APIResponse(APIResponse::Get(key, value));
                        self.network.send(0, msg).await;
                    }
                    cmd => {
                        println!(" Received PUT/DELETE command on Node {}: {:?}", *MY_PID, cmd);
                        // Append the command and check if it fails
                        if let Err(e) = self.omni_paxos.append(cmd) {
                            println!(" Failed to append command on Node {}: {:?}", *MY_PID, e);
                        } else {
                            println!("Successfully appended command on Node {}.", *MY_PID);
                        }
                    }
                },
                Message::OmniPaxosMsg(msg) => {
		    //let leader = self.omni_paxos.get_current_leader();
                    let sender = msg.get_sender();
                    self.heartbeats.insert(sender, Instant::now());
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

    // async fn handle_decided_entries(&mut self) {
    //     let new_decided_idx = self.omni_paxos.get_decided_idx();
    //     if new_decided_idx == 0 {
    //         println!("⚠️ Leader is stuck at Decided(0). This may indicate no commands are being committed.");
    //         return;
    //     }

    //     if self.last_decided_idx < new_decided_idx as u64 {
    //         println!("🔄 New decided index: {} (last_decided_idx: {})", new_decided_idx, self.last_decided_idx);
    
    //         if let Some(decided_entries) = self.omni_paxos.read_decided_suffix(self.last_decided_idx as usize) {
    //             println!(" Applying missing decided entries: {:?}", decided_entries);
    //             self.update_database(decided_entries);
               
    //         } else {
    //             println!(" No decided entries found at index {}. Requesting log sync...", self.last_decided_idx);
    //             if self.omni_paxos.get_current_leader().map(|(leader_id, _)| leader_id) == Some(*MY_PID) {
    //                 if let Err(e) = self.omni_paxos.trim(Some(self.last_decided_idx as usize)) {
    //                     println!(" Log sync failed: {:?}. Skipping trim...", e);
    //                 }
    //             } else {
    //                 println!(" Skipping log sync: Node {} is not the leader!", *MY_PID);
    //             }
        
    //             self.omni_paxos.reconnected(*MY_PID);
    //         }
    //         self.last_decided_idx = new_decided_idx as u64;
    //         // **New Log Trimming Condition**
    //         if self.omni_paxos.get_current_leader().map(|(leader_id, _)| leader_id) == Some(*MY_PID) {
    //             if self.omni_paxos.get_decided_idx() == 0 {
    //                 println!(" Leader is stuck at Decided(0), delaying log trim...");
    //             } else {
    //                 println!(" Ensuring logs are synced before trimming...");
    //                 if let Err(e) = self.omni_paxos.trim(Some(self.last_decided_idx as usize)) {
    //                     println!(" Log trim failed: {:?}. Skipping trimming...", e);
    //                 }
    //             }
    //         } else {
    //             println!(" Skipping log trim: Node {} is not the leader!", *MY_PID);
    //         }
           
    //         // Only send APIResponse when index actually changes
    //         let msg = Message::APIResponse(APIResponse::Decided(new_decided_idx as u64));
    //         self.network.send(0, msg).await;
    //     } 
    
    //     // 🔹 Ensure snapshotting only occurs when log actually advances
    //     if new_decided_idx % 5 == 0 {
    //         match self.omni_paxos.read_decided_suffix(0) {
    //             Some(log) => println!(" Log before snapshot: {:?}", log),
    //             None => println!(" No logs found before snapshot."),
    //         }
    //         self.omni_paxos
    //             .snapshot(Some(new_decided_idx), true)
    //             .expect("Failed to snapshot");
    //         match self.omni_paxos.read_decided_suffix(0) {
    //             Some(log) => println!(" Log after snapshot: {:?}", log),
    //             None => println!(" No logs found after snapshot."),
    //         }
    //     }
    // }

    async fn handle_decided_entries(&mut self) {
        let new_decided_idx = self.omni_paxos.get_decided_idx();
        if new_decided_idx == 0 {
            println!(" Leader is stuck at Decided(0). This may indicate no commands are being committed.");
            return;
        }
    
        if self.last_decided_idx < new_decided_idx as u64 {
            println!(" New decided index: {} (last_decided_idx: {})", new_decided_idx, self.last_decided_idx);
    
            if let Some(decided_entries) = self.omni_paxos.read_decided_suffix(self.last_decided_idx as usize) {
                println!(" Applying missing decided entries: {:?}", decided_entries);
                self.update_database(decided_entries);
            } else {
                println!(" No decided entries found at index {}. Requesting log sync...", self.last_decided_idx);
    
                if self.omni_paxos.get_current_leader().map(|(leader_id, _)| leader_id) == Some(*MY_PID) {
                    println!(" Node is the leader, attempting log sync...");
                    if let Err(e) = self.omni_paxos.trim(Some(self.last_decided_idx as usize)) {
                        println!(" Log sync failed: {:?}. Skipping trim...", e);
                    }
                } else {
                    println!(" Node {} is not the leader! Requesting missing logs from leader...", *MY_PID);
                    self.omni_paxos.reconnected(*MY_PID);
                }
            }
            self.last_decided_idx = new_decided_idx as u64;
    
            if self.omni_paxos.get_decided_idx() > 0 {
                println!(" Ensuring logs are synced before trimming...");
                if let Err(e) = self.omni_paxos.trim(Some(self.last_decided_idx as usize)) {
                    println!(" Log trim failed: {:?}. Skipping trimming...", e);
                }
            } else {
                println!("⚠️ Leader is stuck at Decided(0), skipping log trim.");
            }
    
            let msg = Message::APIResponse(APIResponse::Decided(new_decided_idx as u64));
            self.network.send(0, msg).await;
        }
    
        if new_decided_idx % 20 == 0 {
            match self.omni_paxos.read_decided_suffix(0) {
                Some(log) => println!("📜 Log before snapshot: {:?}", log),
                None => println!("⚠️ No logs found before snapshot."),
            }
            self.omni_paxos
                .snapshot(Some(new_decided_idx), true)
                .expect("Failed to snapshot");
            match self.omni_paxos.read_decided_suffix(0) {
                Some(log) => println!("📜 Log after snapshot: {:?}", log),
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
                _ => {}
            }
        }
    }

    pub(crate) async fn run(&mut self) {
        let mut msg_interval = time::interval(Duration::from_millis(1));
        let mut tick_interval = time::interval(Duration::from_millis(10));
        loop {
            tokio::select! {
                biased;
                _ = msg_interval.tick() => {
                    self.process_incoming_msgs().await;
                    self.send_outgoing_msgs().await;
                    self.handle_decided_entries().await;

                     // Ensure leader election if necessary
                //     if self.omni_paxos.get_current_leader().is_none() {
                //         println!("⚠️ No leader found. Initiating election...");
                //         self.omni_paxos.initiate_election.tick();
                // }


                },
                _ = tick_interval.tick() => {
                    self.omni_paxos.tick();
		            let leader = self.omni_paxos.get_current_leader();
                    if let Some((leader_id, _)) = leader {
                        println!("Node {} is leader.", leader_id);
                    } else {
                        println!("No leader detected.");
                    }
                    let now = Instant::now();
                    let expired_nodes: Vec<u64> = self.heartbeats
                        .iter()
                        .filter(|(_, last_seen)| now.duration_since(**last_seen) >= Duration::from_millis(100))
                        .map(|(sender_id, _)| *sender_id)
                        .collect();

                    // Mark nodes as expired if they aren't already
                    for sender_id in &expired_nodes {
                        if !self.expired_nodes.contains(sender_id) {
                            // println!("Node {} is unresponsive. Marking for reconnection...", sender_id);
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
                        self.heartbeats.remove(&sender_id);
                    }
                        
                    if leader.is_none() {
                        println!("⚠️ No leader detected! Ensuring logs are synced before reconnecting node {}...", *MY_PID);
    
                        if self.omni_paxos.read_decided_suffix(0).is_none() {
                            if self.omni_paxos.get_current_leader().map(|(leader_id, _)| leader_id) == Some(*MY_PID) {
                                if let Err(e) = self.omni_paxos.trim(Some(self.last_decided_idx as usize)) {
                                    println!("❌ Log sync failed: {:?}. Skipping trim...", e);
                                }
                            } else {
                                println!("⚠️ Skipping log sync: Node {} is not the leader!", *MY_PID);
                            }
                        }
    
                        println!("🔄 Reconnecting node {}...", *MY_PID);
                        self.omni_paxos.reconnected(*MY_PID);
                    }
                    
                },
                else => (),
            }
        }
    }
}


