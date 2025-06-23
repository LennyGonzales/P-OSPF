// Fonctions liées à la gestion des messages LSA et routage

// Nettoyage : suppression des imports inutilisés
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use log::{info, warn, error, debug};
use crate::types::{LSAMessage, RouteState};
use crate::error::{AppError, Result};

pub async fn update_topology(state: Arc<crate::AppState>, lsa: &crate::types::LSAMessage) -> crate::error::Result<()> {
    let _current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| crate::error::AppError::ConfigError(e.to_string()))?
        .as_secs();
    let mut topology = state.topology.lock().await;
    topology.insert(
        lsa.originator.clone(),
        crate::types::Router {},
    );
    Ok(())
}

pub async fn send_lsa(
    socket: &tokio::net::UdpSocket,
    addr: &std::net::SocketAddr,
    router_ip: &str,
    last_hop: Option<&str>,
    originator: &str,
    state: std::sync::Arc<crate::AppState>,
    seq_num: u32,
    path: Vec<String>
) -> crate::error::Result<()> {
    let neighbors_guard = state.neighbors.lock().await;
    let neighbors_vec = neighbors_guard.values().cloned().collect::<Vec<_>>();
    drop(neighbors_guard);

    let routing_table_guard = state.routing_table.lock().await;
    let mut route_states = HashMap::new();
    for (dest, (_, state)) in routing_table_guard.iter() {
        route_states.insert(dest.clone(), state.clone());
    }
    drop(routing_table_guard);
    
    // Propager les réseaux selon les bonnes pratiques OSPF
    use pnet::datalink;
    use pnet::ipnetwork::IpNetwork;
    let interfaces = datalink::interfaces();
    let mut has_access_network = false;
    
    for iface in interfaces {
        for ip_network in iface.ips {
            if let IpNetwork::V4(ipv4_network) = ip_network {
                let ip = ipv4_network.ip();
                if !ip.is_loopback() && !ip.is_unspecified() {
                    let network_cidr = ipv4_network.to_string();
                    
                    // - Propager les réseaux de cœur (backbone) : 10.x.x.x
                    // - Propager AUSSI les réseaux d'accès pour la démo : 192.168.x.x
                    if ip.octets()[0] == 10 {
                        route_states.insert(network_cidr.clone(), crate::types::RouteState::Active(0));
                        debug!("Router {} advertising backbone network {}", router_ip, network_cidr);
                    } else if ip.octets()[0] == 192 && ip.octets()[1] == 168 {
                        route_states.insert(network_cidr.clone(), crate::types::RouteState::Active(0));
                        has_access_network = true;
                        debug!("Router {} advertising access network {} (academic demo)", router_ip, network_cidr);
                    }
                }
            }
        }
    }
    
    // Les routeurs d'accès propagent aussi une route par défaut
    if has_access_network {
        route_states.insert("0.0.0.0/0".to_string(), crate::types::RouteState::Active(20));
        debug!("Access router {} advertising default route", router_ip);
    }

    let message = crate::types::LSAMessage {
        message_type: 2,
        router_ip: router_ip.to_string(),
        last_hop: last_hop.map(|s| s.to_string()),
        originator: originator.to_string(),
        seq_num,
        neighbor_count: neighbors_vec.len(),
        neighbors: neighbors_vec,
        routing_table: route_states,
        path,
        ttl: super::INITIAL_TTL,
    };

    let serialized = serde_json::to_vec(&message).map_err(crate::error::AppError::from)?;
    socket.send_to(&serialized, addr).await.map_err(crate::error::AppError::from)?;
    info!("[SEND] LSA from {} (originator: {}, seq: {}) to {}", 
          router_ip, originator, seq_num, addr);
    Ok(())
}

pub async fn forward_lsa(
    socket: &tokio::net::UdpSocket,
    addr: &std::net::SocketAddr,
    router_ip: &str,
    original_lsa: &crate::types::LSAMessage,
    path: Vec<String>,
) -> crate::error::Result<()> {
    if original_lsa.ttl <= 1 {
        return Ok(());
    }
    let message = crate::types::LSAMessage {
        message_type: 2,
        router_ip: router_ip.to_string(),
        last_hop: Some(router_ip.to_string()),
        originator: original_lsa.originator.clone(),
        seq_num: original_lsa.seq_num,
        neighbor_count: original_lsa.neighbor_count,
        neighbors: original_lsa.neighbors.clone(),
        routing_table: original_lsa.routing_table.clone(),
        path,
        ttl: original_lsa.ttl - 1,
    };
    let serialized = serde_json::to_vec(&message).map_err(crate::error::AppError::from)?;
    socket.send_to(&serialized, addr).await.map_err(crate::error::AppError::from)?;
    info!("[FORWARD] LSA from {} (originator: {}, seq: {}) to {}", 
          router_ip, original_lsa.originator, original_lsa.seq_num, addr);
    Ok(())
}

pub async fn update_routing_from_lsa(
    state: std::sync::Arc<crate::AppState>,
    lsa: &crate::types::LSAMessage,
    sender_ip: &str,
    socket: &tokio::net::UdpSocket
) -> crate::error::Result<()> {
    let mut routing_table = state.routing_table.lock().await;
    let next_hop = sender_ip.to_string();
    if lsa.originator != state.local_ip {
        let existing_entry = routing_table.get(&lsa.originator);
        let should_update = match existing_entry {
            Some((_, crate::types::RouteState::Active(current_metric))) => {
                match lsa.routing_table.get(&lsa.originator) {
                    Some(crate::types::RouteState::Active(new_metric)) => new_metric < current_metric,
                    _ => false,
                }
            },
            Some((_, crate::types::RouteState::Unreachable)) => true,
            None => true,
        };
        if should_update {
            let metric = match lsa.routing_table.get(&lsa.originator) {
                Some(crate::types::RouteState::Active(m)) => *m + 1,
                _ => 1,
            };
            routing_table.insert(lsa.originator.clone(), (next_hop.clone(), crate::types::RouteState::Active(metric)));
            info!("Updated route: {} -> next_hop: {} (metric: {})", lsa.originator, next_hop, metric);
            if let Err(e) = update_routing_table_safe(&lsa.originator, &next_hop).await {
                warn!("Could not update system routing table for {}: {}", lsa.originator, e);
            }
        }
    }
    // Traiter les routes depuis la table de routage de la LSA (réseaux uniquement)
    for (dest, route_state) in &lsa.routing_table {
        // Ignorer les routes vers soi-même
        if dest == &state.local_ip {
            continue;
        }
        
        // Ignorer les routes vers des IPs individuelles (ne traiter que les réseaux CIDR)
        if !dest.contains('/') {
            debug!("Skipping individual IP route: {}", dest);
            continue;
        }
        
        // Vérifier si c'est un réseau valide (pas une route vers une IP du même réseau local)
        if let Ok(dest_network) = dest.parse::<pnet::ipnetwork::IpNetwork>() {
            if let pnet::ipnetwork::IpNetwork::V4(dest_net) = dest_network {
                // Vérifier si ce réseau est déjà directement connecté (éviter les doublons)
                let interfaces = pnet::datalink::interfaces();
                let mut is_local = false;
                for iface in interfaces {
                    for ip_network in iface.ips {
                        if let pnet::ipnetwork::IpNetwork::V4(local_net) = ip_network {
                            if dest_net.network() == local_net.network() && dest_net.prefix() == local_net.prefix() {
                                is_local = true;
                                break;
                            }
                        }
                    }
                    if is_local { break; }
                }
                
                if is_local {
                    debug!("Skipping route to local network: {}", dest);
                    continue;
                }
            }
        }
        
        match route_state {
            crate::types::RouteState::Active(metric) => {
                let existing_entry = routing_table.get(dest);
                let new_metric = metric + 1;
                let should_update = match existing_entry {
                    Some((current_next_hop, crate::types::RouteState::Active(current_metric))) => {
                        if new_metric < *current_metric {
                            true
                        } else if new_metric == *current_metric {
                            // Même coût, NE PAS changer de next-hop sauf si l'actuel n'est plus un voisin actif
                            // Vérifier si le next-hop actuel est toujours un voisin actif
                            let neighbors_guard = state.neighbors.lock().await;
                            let still_active = neighbors_guard.get(current_next_hop)
                                .map(|n| n.link_up)
                                .unwrap_or(false);
                            drop(neighbors_guard);
                            !still_active // On met à jour SEULEMENT si l'ancien next-hop n'est plus actif
                        } else {
                            false
                        }
                    },
                    Some((_, crate::types::RouteState::Unreachable)) => true,
                    None => true,
                };
                if should_update {
                    routing_table.insert(dest.clone(), (next_hop.clone(), RouteState::Active(new_metric)));
                    info!("Learned network route from LSA: {} -> next_hop: {} (metric: {})", dest, next_hop, new_metric);
                    if let Err(e) = update_routing_table_safe(dest, &next_hop).await {
                        warn!("Could not update system routing table for {}: {}", dest, e);
                    }
                } else {
                    debug!("Route to {} not updated (same or worse metric, or would cause flapping)", dest);
                }
            },
            crate::types::RouteState::Unreachable => {
                if let Some((current_next_hop, _)) = routing_table.get(dest) {
                    if current_next_hop == &next_hop {
                        routing_table.insert(dest.clone(), (next_hop.clone(), crate::types::RouteState::Unreachable));
                        info!("Network route marked as unreachable: {}", dest);
                    }
                }
            }
        }
    }
    Ok(())
}

pub async fn send_poisoned_route(
    socket: &tokio::net::UdpSocket,
    addr: &std::net::SocketAddr,
    router_ip: &str,
    poisoned_route: &str,
    seq_num: u32,
    path: Vec<String>
) -> crate::error::Result<()> {
    let mut routing_table = HashMap::new();
    routing_table.insert(poisoned_route.to_string(), crate::types::RouteState::Unreachable);
    let message = crate::types::LSAMessage {
        message_type: 2,
        router_ip: router_ip.to_string(),
        last_hop: None,
        originator: router_ip.to_string(),
        seq_num,
        neighbor_count: 0,
        neighbors: Vec::new(),
        routing_table,
        path,
        ttl: super::INITIAL_TTL,
    };
    let serialized = serde_json::to_vec(&message).map_err(crate::error::AppError::from)?;
    socket.send_to(&serialized, addr).await.map_err(crate::error::AppError::from)?;
    info!("[SEND] POISON ROUTE for {} from {} to {}", poisoned_route, router_ip, addr);
    Ok(())
}

pub async fn update_routing_table_safe(destination: &str, gateway: &str) -> crate::error::Result<()> {
    use pnet::ipnetwork::IpNetwork;
    use pnet::datalink;
    
    // Vérifier si c'est une route vers un réseau (CIDR) ou une IP individuelle
    if !destination.contains('/') {
        debug!("Skipping route to individual IP (not a network): {}", destination);
        return Ok(());
    }
    
    let network: IpNetwork = destination.parse()
        .map_err(|e| crate::error::AppError::RouteError(format!("Invalid destination network {}: {}", destination, e)))?;
    let gateway_ip: Ipv4Addr = gateway.parse()
        .map_err(|e| crate::error::AppError::RouteError(format!("Invalid gateway IP {}: {}", gateway, e)))?;
    if gateway_ip.is_loopback() || gateway_ip.is_unspecified() {
        debug!("Skipping route to invalid gateway: {} via {}", destination, gateway);
        return Ok(());
    }
    // Vérifier que la gateway est directement accessible (sur un réseau local)
    let interfaces = datalink::interfaces();
    let mut gateway_is_local = false;
    let mut local_networks = Vec::new();
    
    for iface in interfaces {
        for ip_network in iface.ips {
            if let IpNetwork::V4(ipv4_network) = ip_network {
                local_networks.push(ipv4_network.to_string());
                if ipv4_network.contains(gateway_ip) {
                    debug!("Gateway {} found in local network {}", gateway_ip, ipv4_network);
                    gateway_is_local = true;
                    break;
                }
            }
        }
        if gateway_is_local { break; }
    }
    
    if !gateway_is_local {
        debug!("Gateway {} is not in any local networks: {:?}", gateway, local_networks);
        debug!("Skipping route to {} via non-local gateway {}", destination, gateway);
        return Ok(());
    }
    
    // Vérification supplémentaire : éviter d'ajouter une route vers son propre réseau
    if let IpNetwork::V4(dest_net) = network {
        for iface in datalink::interfaces() {
            for ip_network in iface.ips {
                if let IpNetwork::V4(local_net) = ip_network {
                    if dest_net.network() == local_net.network() && dest_net.prefix() == local_net.prefix() {
                        debug!("Skipping route to local network {} via {}", destination, gateway);
                        return Ok(());
                    }
                }
            }
        }
    }
    let handle = net_route::Handle::new()
        .map_err(|e| crate::error::AppError::RouteError(format!("Cannot create routing handle (permissions?): {}", e)))?;
    let (ip, prefix) = match network {
        IpNetwork::V4(net) => (IpAddr::V4(net.network()), net.prefix()),
        IpNetwork::V6(_) => {
            return Err(crate::error::AppError::RouteError("IPv6 not supported".to_string()));
        }
    };
    let route = net_route::Route::new(ip, prefix as u8)
        .with_gateway(IpAddr::V4(gateway_ip));
    match handle.add(&route).await {
        Ok(_) => {
            info!("Successfully added network route to {} via {}", destination, gateway_ip);
            Ok(())
        },
        Err(e) => {
            debug!("Route add failed, trying to update: {}", e);
            let _ = handle.delete(&route).await;
            match handle.add(&route).await {
                Ok(_) => {
                    info!("Successfully updated network route to {} via {}", destination, gateway_ip);
                    Ok(())
                },
                Err(e2) => {
                    warn!("Failed to add/update route to {} via {}: {}", destination, gateway_ip, e2);
                    Err(crate::error::AppError::RouteError(format!("Routing update failed: {}", e2)))
                }
            }
        }
    }
}
