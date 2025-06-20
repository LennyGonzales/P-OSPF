// Fonctions liées à la gestion des voisins

use std::sync::Arc;
use tokio::net::UdpSocket;
use log::{info, warn, error};
use crate::AppState;
use std::time::Duration;

use crate::types::{Neighbor};
use crate::net_utils::get_broadcast_addresses;

pub async fn update_neighbor(state: &Arc<crate::AppState>, neighbor_ip: &str) {
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs();
    let mut neighbors = state.neighbors.lock().await;
    neighbors.entry(neighbor_ip.to_string())
        .and_modify(|n| {
            n.last_seen = current_time;
            if !n.link_up {
                info!("Neighbor {} is now UP", neighbor_ip);
                n.link_up = true;
            }
        })
        .or_insert_with(|| {
            info!("New neighbor discovered: {}", neighbor_ip);
            crate::types::Neighbor {
                neighbor_ip: neighbor_ip.to_string(),
                link_up: true,
                capacity: 100,
                last_seen: current_time,
            }
        });
}

pub async fn check_neighbor_timeouts(state: &Arc<AppState>) {
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs();
    let mut neighbors = state.neighbors.lock().await;
    let mut changed = false;
    for (ip, neighbor) in neighbors.iter_mut() {
        if neighbor.link_up && current_time - neighbor.last_seen > super::NEIGHBOR_TIMEOUT_SEC {
            warn!("Neighbor {} is DOWN (timeout)", ip);
            neighbor.link_up = false;
            changed = true;
        }
    }
    drop(neighbors);
    if changed {
        let broadcast_addrs = get_broadcast_addresses(super::PORT);
        let socket = UdpSocket::bind("0.0.0.0:0").await.unwrap_or_else(|_| panic!("Failed to create socket"));
        socket.set_broadcast(true).unwrap_or_else(|_| panic!("Failed to set broadcast"));
        for (local_ip, addr) in &broadcast_addrs {
            let seq_num = current_time as u32;
            if let Err(e) = super::send_lsa(&socket, addr, local_ip, None, local_ip, Arc::clone(&state), seq_num, vec![]).await {
                error!("Failed to send LSA after neighbor timeout: {}", e);
            }
        }
    }
}
