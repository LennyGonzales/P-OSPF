// Fonctions liées à la gestion des messages LSA et routage

// Nettoyage : suppression des imports inutilisés
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use log::{info, warn, error, debug};
use crate::types::{LSAMessage, RouteState};
use crate::error::{AppError, Result};

pub async fn update_topology(state: Arc<crate::AppState>, lsa: &crate::types::LSAMessage) -> Result<()> {
    let mut topology = state.topology.lock().await;

    let router_state = topology.entry(lsa.originator.clone()).or_insert_with(crate::types::Router::new);

    // Ne mettre à jour que si le nouveau LSA est plus récent
    if router_state.last_lsa.as_ref().map_or(true, |old_lsa| lsa.seq_num > old_lsa.seq_num) {
        router_state.last_lsa = Some(lsa.clone());
        debug!("Updated topology for originator {}", lsa.originator);
    }
    
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
) -> Result<()> {
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

    crate::net_utils::send_message(socket, addr, &message, state.key.as_slice(),"[SEND] LSA").await
}

pub async fn forward_lsa(
    socket: &tokio::net::UdpSocket,
    _broadcast_addr: &std::net::SocketAddr, // ignoré, on envoie unicast à chaque voisin
    local_ip: &str,
    original_lsa: &crate::types::LSAMessage,
    mut path: Vec<String>,
    state: &std::sync::Arc<crate::AppState>,
) -> Result<()> {
    if original_lsa.ttl <= 1 {
        return Ok(());
    }

    // Ajoute notre IP au chemin
    if !path.contains(&local_ip.to_string()) {
        path.push(local_ip.to_string());
    }

    let neighbors = state.neighbors.lock().await;
    for (neighbor_ip, neighbor) in neighbors.iter() {
        // Ne pas relayer à soi-même, ni à l'expéditeur direct (last_hop)
        if neighbor_ip == local_ip {
            continue;
        }
        if let Some(last_hop) = &original_lsa.last_hop {
            if neighbor_ip == last_hop {
                continue;
            }
        }
        if !neighbor.link_up {
            continue;
        }
        // Empêcher la boucle : ne pas relayer si déjà dans le path
        if path.contains(neighbor_ip) {
            continue;
        }
        // Calculer l'adresse du voisin
        let addr = format!("{}:{}", neighbor_ip, crate::PORT)
            .parse::<std::net::SocketAddr>()
            .map_err(|e| AppError::NetworkError(format!("Invalid neighbor addr: {}", e)))?;

        // Préparer le LSA à relayer
        let message = crate::types::LSAMessage {
            message_type: 2,
            router_ip: local_ip.to_string(),
            last_hop: Some(local_ip.to_string()),
            originator: original_lsa.originator.clone(),
            seq_num: original_lsa.seq_num,
            neighbor_count: original_lsa.neighbor_count,
            neighbors: original_lsa.neighbors.clone(),
            routing_table: original_lsa.routing_table.clone(),
            path: path.clone(),
            ttl: original_lsa.ttl - 1,
        };

        crate::net_utils::send_message(socket, &addr, &message, state.key.as_slice(), "[FORWARD]").await?;
        info!("[FORWARD] LSA from {} (originator: {}, seq: {}) to {}", 
              local_ip, original_lsa.originator, original_lsa.seq_num, addr);
    }
    Ok(())
}

pub async fn update_routing_from_lsa(
    state: std::sync::Arc<crate::AppState>,
    lsa: &crate::types::LSAMessage,
    _sender_ip: &str,
    _socket: &tokio::net::UdpSocket
) -> Result<()> {
    // Appeler le recalcul global des routes à chaque réception de LSA
    crate::dijkstra::calculate_and_update_optimal_routes(std::sync::Arc::clone(&state)).await
}

pub async fn send_poisoned_route(
    socket: &tokio::net::UdpSocket,
    addr: &std::net::SocketAddr,
    router_ip: &str,
    poisoned_route: &str,
    seq_num: u32,
    path: Vec<String>,
    state: &std::sync::Arc<crate::AppState>, // Ajout du state pour accéder à la clé
) -> Result<()> {
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
    
    crate::net_utils::send_message(socket, addr, &message, state.key.as_slice(), "[POISON]").await?;
    info!("[SEND] POISON ROUTE for {} from {} to {}", poisoned_route, router_ip, addr);
    Ok(())
}

pub async fn update_routing_table_safe(destination: &str, gateway: &str) -> Result<()> {
    use pnet::ipnetwork::IpNetwork;
    use pnet::datalink;
    
    // Vérifier si c'est une route vers un réseau (CIDR) ou une IP individuelle
    if !destination.contains('/') {
        debug!("Skipping route to individual IP (not a network): {}", destination);
        return Ok(());
    }
    
    let network: IpNetwork = destination.parse()
        .map_err(|e| AppError::RouteError(format!("Invalid destination network {}: {}", destination, e)))?;
    let gateway_ip: Ipv4Addr = gateway.parse()
        .map_err(|e| AppError::RouteError(format!("Invalid gateway IP {}: {}", gateway, e)))?;
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
        .map_err(|e| AppError::RouteError(format!("Cannot create routing handle (permissions?): {}", e)))?;
    let (ip, prefix) = match network {
        IpNetwork::V4(net) => (IpAddr::V4(net.network()), net.prefix()),
        IpNetwork::V6(_) => {
            return Err(AppError::RouteError("IPv6 not supported".to_string()));
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
                    Err(AppError::RouteError(format!("Routing update failed: {}", e2)))
                }
            }
        }
    }
}
