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
    
    // Ajouter tous les réseaux locaux (interfaces directes) dans la LSA
    use pnet::datalink;
    use pnet::ipnetwork::IpNetwork;
    let interfaces = datalink::interfaces();
    for iface in interfaces {
        for ip_network in iface.ips {
            if let IpNetwork::V4(ipv4_network) = ip_network {
                let ip = ipv4_network.ip();
                if !ip.is_loopback() && !ip.is_unspecified() {
                    let network_cidr = ipv4_network.to_string();
                    // Ajouter ce réseau local comme directement accessible (métrique 0)
                    route_states.insert(network_cidr, crate::types::RouteState::Active(0));
                }
            }
        }
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
    for neighbor in &lsa.neighbors {
        if neighbor.link_up {
            if neighbor.neighbor_ip == state.local_ip {
                continue;
            }
            // Utiliser le réseau de l'interface comme clé (ex: 10.2.0.0/24)
            // À adapter : il faut que le LSA transporte le préfixe réseau du voisin
            // Pour l'instant, on suppose que neighbor.neighbor_ip est déjà un préfixe réseau CIDR
            let network_prefix = &neighbor.neighbor_ip; // Ex: "10.2.0.0/24"
            let existing_entry = routing_table.get(network_prefix);
            let neighbor_metric = (100 / neighbor.capacity.max(1)) as u32;
            let should_update = match existing_entry {
                Some((_, crate::types::RouteState::Active(current_metric))) => {
                    neighbor_metric + 1 < *current_metric
                },
                Some((_, crate::types::RouteState::Unreachable)) => true,
                None => true,
            };
            if should_update {
                routing_table.insert(network_prefix.clone(), 
                                  (next_hop.clone(), crate::types::RouteState::Active(neighbor_metric + 1)));
                info!("Updated route: {} -> next_hop: {} (metric: {})", 
                      network_prefix, next_hop, neighbor_metric + 1);
                if let Err(e) = update_routing_table_safe(network_prefix, &next_hop).await {
                    warn!("Could not update system routing table for {}: {}", network_prefix, e);
                }
            }
        } else if routing_table.contains_key(&neighbor.neighbor_ip) {
            info!("Route poisoning: {} -> unreachable", neighbor.neighbor_ip);
            routing_table.insert(neighbor.neighbor_ip.clone(), 
                              (next_hop.clone(), crate::types::RouteState::Unreachable));
            let broadcast_addrs = crate::net_utils::get_broadcast_addresses(super::PORT);
            for (local_ip, addr) in &broadcast_addrs {
                let seq_num = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_else(|_| std::time::Duration::from_secs(0))
                    .as_secs() as u32;
                let path = vec![state.local_ip.clone()];
                if let Err(e) = send_poisoned_route(socket, addr, local_ip, &neighbor.neighbor_ip, 
                                                  seq_num, path).await {
                    error!("Failed to send poisoned route: {}", e);
                }
            }
        }
    }
    for (dest, route_state) in &lsa.routing_table {
        if dest == &state.local_ip {
            continue;
        }
        match route_state {
            crate::types::RouteState::Active(metric) => {
                let existing_entry = routing_table.get(dest);
                let new_metric = metric + 1;
                let should_update = match existing_entry {
                    Some((_, crate::types::RouteState::Active(current_metric))) => new_metric < *current_metric,
                    Some((_, crate::types::RouteState::Unreachable)) => true,
                    None => true,
                };
                if should_update {
                    routing_table.insert(dest.clone(), (next_hop.clone(), RouteState::Active(new_metric)));
                    info!("Learned route from LSA: {} -> next_hop: {} (metric: {})", dest, next_hop, new_metric);
                    if let Err(e) = update_routing_table_safe(dest, &next_hop).await {
                        warn!("Could not update system routing table for {}: {}", dest, e);
                    }
                }
            },
            crate::types::RouteState::Unreachable => {
                if let Some((current_next_hop, _)) = routing_table.get(dest) {
                    if current_next_hop == &next_hop {
                        routing_table.insert(dest.clone(), (next_hop.clone(), crate::types::RouteState::Unreachable));
                        info!("Route marked as unreachable: {}", dest);
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
