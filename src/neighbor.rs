// Fonctions liées à la gestion des voisins

use std::sync::Arc;
use tokio::net::UdpSocket;
use log::{info, warn, error};
use crate::AppState;
use std::time::Duration;

use crate::net_utils::get_broadcast_addresses;

pub async fn update_neighbor(state: &Arc<crate::AppState>, neighbor_ip: &str) {
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs();
    
    // Obtenir les informations de l'interface pour ce voisin
    let (capacity, link_active) = get_interface_info_for_neighbor(state, neighbor_ip).await;
    
    let mut neighbors = state.neighbors.lock().await;
    neighbors.entry(neighbor_ip.to_string())
        .and_modify(|n| {
            n.last_seen = current_time;
            n.capacity = capacity;
            // Le lien n'est considéré comme UP que si l'interface est active ET le voisin répond
            let should_be_up = link_active && true; // true car on a reçu un message du voisin
            if n.link_up != should_be_up {
                if should_be_up {
                    info!("Neighbor {} is now UP (capacity: {} Mbps)", neighbor_ip, capacity);
                } else {
                    warn!("Neighbor {} is now DOWN (interface inactive)", neighbor_ip);
                }
                n.link_up = should_be_up;
            }
        })
        .or_insert_with(|| {
            let should_be_up = link_active;
            if should_be_up {
                info!("New neighbor discovered: {} (capacity: {} Mbps)", neighbor_ip, capacity);
            } else {
                warn!("New neighbor discovered but interface is DOWN: {}", neighbor_ip);
            }
            crate::types::Neighbor {
                neighbor_ip: neighbor_ip.to_string(),
                link_up: should_be_up,
                capacity,
                last_seen: current_time,
            }
        });
    
    // Déclencher un recalcul des routes si c'est un nouveau voisin ou un changement d'état
    let state_clone = Arc::clone(state);
    tokio::spawn(async move {
        // Attendre un peu pour que les changements se stabilisent
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        if let Err(e) = crate::dijkstra::calculate_and_update_optimal_routes(state_clone).await {
            log::warn!("Échec du recalcul des routes après changement de voisin: {}", e);
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

/// Détermine la capacité et l'état d'une interface pour un voisin donné
async fn get_interface_info_for_neighbor(state: &Arc<AppState>, neighbor_ip: &str) -> (u32, bool) {
    // Pour l'instant, on utilise la première interface active configurée
    // Dans une implémentation plus avancée, on pourrait déterminer l'interface
    // en fonction de l'adresse IP du voisin et des réseaux configurés
    
    for interface in &state.config.interfaces {
        if interface.link_active {
            return (interface.capacity_mbps, true);
        }
    }
    
    // Si aucune interface active, utiliser la première interface disponible
    if let Some(interface) = state.config.interfaces.first() {
        (interface.capacity_mbps, interface.link_active)
    } else {
        (100, false) // Valeurs par défaut
    }
}

/// Affiche un rapport détaillé de l'état des interfaces
pub async fn display_interface_report(state: &Arc<AppState>) {
    use log::info;
    
    info!("=== RAPPORT D'ÉTAT DES INTERFACES ===");
    
    if state.config.interfaces.is_empty() {
        info!("Aucune interface configurée");
        return;
    }
    
    info!("{:<10} {:<12} {:<8} {:<10}", "Interface", "Capacité", "État", "Coût OSPF");
    info!("{}", "-".repeat(45));
    
    for interface in &state.config.interfaces {
        let status = if interface.link_active { "ACTIF" } else { "INACTIF" };
        let cost = if interface.link_active {
            // Calculer le coût OSPF (100 Mbps de référence)
            let reference_bandwidth = 100_000_000;
            let bandwidth_bps = interface.capacity_mbps * 1_000_000;
            let cost = reference_bandwidth / bandwidth_bps;
            cost.max(1)
        } else {
            u32::MAX
        };
        
        let cost_str = if cost == u32::MAX {
            "∞".to_string()
        } else {
            cost.to_string()
        };
        
        info!("{:<10} {:<12} {:<8} {:<10}", 
              interface.name, 
              format!("{} Mbps", interface.capacity_mbps),
              status,
              cost_str);
    }
    
    // Statistiques générales
    let total_interfaces = state.config.interfaces.len();
    let active_interfaces = state.config.interfaces.iter()
        .filter(|iface| iface.link_active)
        .count();
    
    info!("Total interfaces: {} (actives: {})", total_interfaces, active_interfaces);
    
    // Capacité totale disponible
    let total_capacity: u32 = state.config.interfaces.iter()
        .filter(|iface| iface.link_active)
        .map(|iface| iface.capacity_mbps)
        .sum();
    
    info!("Capacité totale disponible: {} Mbps", total_capacity);
}
