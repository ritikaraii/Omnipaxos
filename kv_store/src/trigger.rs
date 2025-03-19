use kube::{api::{Api, ListParams}, Client};
use k8s_openapi::api::core::v1::Pod;
use crate::{NODES, PID, CONFIG_ID};
use crate::server::Server;
use omnipaxos::ClusterConfig;
use std::time::Duration;
use tokio::time::sleep;

/// Poll the cluster for pods and return the PID of a new pod if found.
/// This function lists pods in the default namespace (without using a watcher) and checks
/// for pods whose name indicates a new node (e.g. "kv-store-<id>").

pub struct Trigger {}

impl Trigger {
    pub async fn new() -> Self {
        Trigger {}
    }


// pub async fn poll_for_new_pod() -> Option<u64> {
//     // Create a Kubernetes client.
//     let client = Client::try_default().await.unwrap();
//     // Get pods in the default namespace.
//     let pods: Api<Pod> = Api::default_namespaced(client);
//     let pod_list = pods.list(&ListParams::default()).await.ok()?;

//     // Build a list of peer PIDs from our constant NODES (excluding our own PID).
//     let peers: Vec<u64> = NODES.iter().filter(|pid| **pid != *PID).cloned().collect();

//     // Look for a pod whose name starts with "kv-store" and does not belong to our current peer list.
//     for pod in pod_list.items {
//         if let Some(pod_name) = pod.metadata.name {
//             if pod_name.starts_with("kv-store") && pod_name != format!("kv-store-{}", *PID - 1) {
//                 let parts: Vec<&str> = pod_name.split('-').collect();
//                 if parts.len() < 3 {
//                     println!("Skipping pod {}: Invalid name format", pod_name);
//                     continue;
//                 }
//                 // Assume the pod name is of the form "kv-store-<id>".
//                 if let Ok(parsed_pid) = parts[2].parse::<u64>() {
//                     let new_pid = parsed_pid + 1; // Ensure non-zero.
//                     if !peers.contains(&new_pid) {
//                         println!("Detected pod {} with new PID {} (not in peer list).", pod_name, new_pid);
//                         return Some(new_pid);
//                     }
//                 } else {
//                     println!("Failed to parse PID from pod name: {}", pod_name);
//                 }
//             }
//         }
//     }
//     None
// }

// pub async fn poll_for_new_pod() -> Option<u64> {
//     let client = Client::try_default().await.unwrap();
//     let pods: Api<Pod> = Api::default_namespaced(client);
//     let pod_list = pods.list(&ListParams::default()).await.ok()?;

//     let mut max_pid = *NODES.iter().max().unwrap_or(&0);

//     for pod in pod_list.items {
//         if let Some(pod_name) = pod.metadata.name {
//             if (pod_name.starts_with("kv-store") && *pod_name != format!("kv-store-{}", *PID - 1)) {
//                 let parts: Vec<&str> = pod_name.split('-').collect();
//                 if parts.len() < 3 {
//                     continue;
//                 }
//                 if let Ok(parsed_pid) = parts[2].parse::<u64>() {
//                     if parsed_pid > max_pid {
//                         println!("Parsed Pid {:?}", parsed_pid);
//                         max_pid = parsed_pid;
//                     }
//                 }
//             }
//         }
//     }

//     if max_pid > *NODES.iter().max().unwrap_or(&0) {
//         Some(max_pid)
//     } else {
//         None
//     }
// }

// /// Polls for new pods every `interval` seconds, and if a new pod is found,
// /// triggers a reconfiguration on the provided server.
// pub(crate) async fn poll_and_trigger_reconfig(server: &mut Server) {
//     loop {
//         if let Some(new_pid) = Self::poll_for_new_pod().await {
//             // If a new pod (new PID) is detected, update the node list.
//             println!("New pod detected with PID {}. Triggering reconfiguration.", new_pid);
//             let mut updated_nodes = NODES.clone();
//             updated_nodes.push(new_pid);
//             let new_config_id = *CONFIG_ID + 1; // Assuming you can get current config.
//             let new_cluster = ClusterConfig {
//                 configuration_id: new_config_id,
//                 nodes: updated_nodes,
//                 ..Default::default()
//             };

//             // Lock the server and trigger reconfiguration.
//             println!(
//                 "Triggering reconfiguration: new config id: {}, new nodes: {:?}",
//                 new_config_id, new_cluster.nodes
//             );
//             if let Err(e) = server.omni_paxos.reconfigure(new_cluster, None) {
//                 println!("⚠️ Reconfiguration failed: {:?}", e);
//             } else {
//                 println!("✅ Reconfiguration successful. Requesting log sync for new nodes...");
//                 server.request_log_sync_from_leader().await;
//             }
//         }
//         // Wait for the next poll.
//         sleep(Duration::from_secs(5)).await;
//     }
// }


    /// Polls the Kubernetes cluster for new pods and determines if a new node has been added.
    pub async fn poll_for_new_pod() -> Option<u64> {
        let client = match Client::try_default().await {
            Ok(c) => c,
            Err(e) => {
                println!(" Failed to create Kubernetes client: {:?}", e);
                return None;
            }
        };

        let pods: Api<Pod> = Api::default_namespaced(client);
        let pod_list = match pods.list(&ListParams::default()).await {
            Ok(p) => p,
            Err(e) => {
                println!(" Failed to list pods: {:?}", e);
                return None;
            }
        };

        //let mut max_pid = *NODES.iter().max().unwrap_or(&0);#
        let pods: Vec<u64> = NODES
        .iter()
        .filter(|pid| **pid != *PID)
        .cloned()
        .collect();
        let mut detected_pid = None;

        for pod in pod_list.items {
            if let Some(pod_name) = pod.metadata.name {
                if pod_name.starts_with("kv-store") && *pod_name != format!("kv-store-{}", *PID - 1) {
                    let parts: Vec<&str> = pod_name.split('-').collect();
                    if parts.len() < 3 {
                        println!(" Skipping pod {}: Invalid name format", pod_name);
                        continue;
                    }
                    
                    if let Ok(parsed_pid) = parts[2].parse::<u64>() {
                        let new_pid = parsed_pid + 1 ; // Adjust PID to be 1-based
                        if pods.contains(&new_pid) {
                            println!(" Detected new pod: {} with PID {}", pod_name, new_pid);
                            detected_pid = Some(new_pid);
                        }
                    } else {
                        println!(" Failed to parse PID from pod name: {}", pod_name);
                    }
                }
            }
        }

        detected_pid
    }

    /// Periodically checks for new nodes and triggers a reconfiguration if a new pod is detected.
    pub(crate) async fn poll_and_trigger_reconfig(server: &mut Server) {
        loop {
            match Self::poll_for_new_pod().await {
                Some(new_pid) => {
                    println!(" New pod detected with PID {}. Initiating reconfiguration...", new_pid);
    
                    let mut updated_nodes = NODES.clone();
                    
                    if !updated_nodes.contains(&new_pid) {
                        updated_nodes.push(new_pid);
                        println!(" Node {} added to cluster: {:?}", new_pid, updated_nodes);
                    } else {
                        println!(" Node {} already exists in cluster.", new_pid);
                    }
    
                    let updated_nodes_clone = updated_nodes.clone(); // Fix Borrowing Issue
                    let new_config_id = *CONFIG_ID + 1; 
    
                    let new_cluster = ClusterConfig {
                        configuration_id: new_config_id,
                        nodes: updated_nodes_clone,
                        ..Default::default()
                    };
    
                    match server.omni_paxos.reconfigure(new_cluster, None) {
                        Ok(_) => {
                            println!(" Reconfiguration successful! New nodes: " );
                            server.request_log_sync_from_leader().await; // Ensure logs sync after reconfig
                        }
                        Err(e) => println!(" Reconfiguration failed: {:?}", e),
                    }
                }
                None => {
                    println!(" No new pods detected. Retrying in 5 seconds...");
                }
            }
    
            sleep(Duration::from_secs(5)).await;
        }
    }
}


