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
