use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use std::sync::Arc;
use net_route::{Route, Handle};
use pnet::datalink::{self, NetworkInterface};

#[derive(Debug, Serialize, Deserialize, Clone)]
struct HelloMessage {
    message_type: u8,
    router_ip: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Neighbor {
    neighbor_ip: String,
    link_up: bool,
    capacity: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct LSAMessage {
    message_type: u8,
    router_ip: String,
    neighbor_count: usize,
    neighbors: Vec<Neighbor>,
}

struct Router {
    router_ip: String,
    neighbors: Vec<Neighbor>,
}

struct AppState {
    topology: Mutex<HashMap<String, Router>>,
    neighbors: Mutex<HashMap<String, Neighbor>>,
}

fn get_broadcast_addresses_with_local(port: u16) -> Vec<(String, SocketAddr)> {
    let interfaces = datalink::interfaces();
    interfaces
        .into_iter()
        .flat_map(|iface: NetworkInterface| {
            iface.ips.into_iter().filter_map(move |ip_network| {
                if let IpAddr::V4(ip) = ip_network.ip() {
                    let prefix_len = ip_network.prefix();
                    let mask = u32::MAX << (32 - prefix_len);
                    let broadcast = u32::from(ip) | !mask;
                    Some((ip.to_string(), SocketAddr::new(IpAddr::V4(Ipv4Addr::from(broadcast)), port)))
                } else {
                    None
                }
            })
        })
        .collect()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let socket = Arc::new(UdpSocket::bind("0.0.0.0:5000").await?);
    socket.set_broadcast(true)?;

    let broadcast_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(255, 255, 255, 255)), 5000);

    let state = Arc::new(AppState {
        topology: Mutex::new(HashMap::new()),
        neighbors: Mutex::new(HashMap::new()),
    });

    let socket_clone = Arc::clone(&socket);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(20)).await;
            let broadcast_addrs = get_broadcast_addresses_with_local(5000);

            for (local_ip, addr) in &broadcast_addrs {
                if let Err(e) = send_hello(&socket_clone, addr, local_ip).await {
                    log::error!("Failed to send hello to {}: {}", addr, e);
                }
            }
        }
    });

    let mut buf = [0; 2048];

    // Récupère toutes les IP locales (IPv4)
    let local_ips: Vec<IpAddr> = datalink::interfaces()
        .into_iter()
        .flat_map(|iface| iface.ips)
        .filter_map(|ip_network| {
            if let IpAddr::V4(ipv4) = ip_network.ip() {
                Some(IpAddr::V4(ipv4))
            } else {
                None
            }
        })
        .collect();

    loop {
        let (len, src_addr) = socket.recv_from(&mut buf).await?;
        // Ignore les paquets venant d'une IP locale
        if local_ips.contains(&src_addr.ip()) {
            continue;
        }
        println!("Received {} bytes from {}", len, src_addr);

        match serde_json::from_slice::<serde_json::Value>(&buf[..len]) {
            Ok(json) => {
                if let Some(message_type) = json.get("message_type").and_then(|v| v.as_u64()) {
                    match message_type {
                        1 => {
                            if let Ok(hello) = serde_json::from_value::<HelloMessage>(json) {
                                println!("[RECV] HELLO from {} - {}", hello.router_ip, src_addr);

                                let mut neighbors = state.neighbors.lock().await;
                                neighbors.insert(
                                    hello.router_ip.clone(),
                                    Neighbor {
                                        neighbor_ip: hello.router_ip.clone(),
                                        link_up: true,
                                        capacity: 100,
                                    },
                                );
                                drop(neighbors);

                                // Get the local address of the interface that received the Hello message
                                let mut found_local_ip = None;
                                if let std::net::SocketAddr::V4(src_v4) = src_addr {
                                    let interfaces = pnet::datalink::interfaces();
                                    for iface in interfaces {
                                        for ip_network in iface.ips {
                                            if let IpAddr::V4(local_ip) = ip_network.ip() {
                                                // Vérifie si src_addr et local_ip sont sur le même réseau /24
                                                if (u32::from(*src_v4.ip()) & 0xFFFFFF00) == (u32::from(local_ip) & 0xFFFFFF00) {
                                                    found_local_ip = Some(local_ip.to_string());
                                                    break;
                                                }
                                            }
                                        }
                                        if found_local_ip.is_some() { break; }
                                    }
                                }
                                if let Some(local_ip_str) = found_local_ip {
                                    if let Err(e) = send_lsa(&socket, &broadcast_addr, &local_ip_str, state.clone()).await {
                                        log::error!("Failed to send LSA: {}", e);
                                    }
                                } else {
                                    log::warn!("No matching local interface found for src_addr {}", src_addr);
                                }
                            }
                        }
                        2 => {
                            if let Ok(lsa) = serde_json::from_value::<LSAMessage>(json) {
                                println!("[RECV] LSA from {} - {}", lsa.router_ip, src_addr);

                                if let Err(e) = update_topology(state.clone(), &lsa).await {
                                    log::error!("Failed to update topology: {}", e);
                                }

                                // Utilise l'IP locale de l'interface pour le calcul des chemins
                                if let Ok(local_addr) = socket.local_addr() {
                                    let local_ip_str = match local_addr.ip() {
                                        std::net::IpAddr::V4(ipv4) => ipv4.to_string(),
                                        std::net::IpAddr::V6(ipv6) => ipv6.to_string(),
                                    };
                                    if let Err(e) = compute_shortest_paths(state.clone(), &local_ip_str).await {
                                        log::error!("Failed to compute shortest paths: {}", e);
                                    }
                                }
                            }
                        }
                        _ => log::warn!("Unknown message type: {}", message_type),
                    }
                } else {
                    log::warn!("No message_type field in received JSON");
                }
            }
            Err(e) => {
                log::error!("Failed to parse JSON: {}", e);
            }
        }
    }
}

async fn send_hello(socket: &UdpSocket, addr: &SocketAddr, router_ip: &str) -> Result<(), Box<dyn std::error::Error>> {
    let message = HelloMessage {
        message_type: 1,
        router_ip: router_ip.to_string(),
    };
    let serialized = serde_json::to_vec(&message)?;
    socket.send_to(&serialized, addr).await?;
    println!("[SEND] HELLO from {} - {}", router_ip, addr);
    Ok(())
}

async fn send_lsa(socket: &UdpSocket, addr: &SocketAddr, router_ip: &str, state: Arc<AppState>) -> Result<(), Box<dyn std::error::Error>> {
    let neighbors = state.neighbors.lock().await;
    let neighbors_vec = neighbors.values().cloned().collect::<Vec<_>>();

    let message = LSAMessage {
        message_type: 2,
        router_ip: router_ip.to_string(),
        neighbor_count: neighbors_vec.len(),
        neighbors: neighbors_vec,
    };

    let serialized = serde_json::to_vec(&message)?;
    socket.send_to(&serialized, addr).await?;
    println!("[SEND] LSA from {}", router_ip);
    Ok(())
}

fn get_broadcast_addresses(port: u16) -> Vec<SocketAddr> {
    let interfaces = datalink::interfaces();
    interfaces
        .into_iter()
        .flat_map(|iface: NetworkInterface| {
            iface.ips.into_iter().filter_map(move |ip_network| {
                if let IpAddr::V4(ip) = ip_network.ip() {
                    let prefix_len = ip_network.prefix();
                    let mask = u32::MAX << (32 - prefix_len);
                    let broadcast = u32::from(ip) | !mask;
                    Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::from(broadcast)), port))
                } else {
                    None
                }
            })
        })
        .collect()
}

fn get_local_ip() -> Result<String, Box<dyn std::error::Error>> {
    let interfaces = datalink::interfaces();
    for interface in interfaces {
        for ip_network in interface.ips {
            if let IpAddr::V4(ipv4) = ip_network.ip() {
                if !ipv4.is_loopback() && !ipv4.is_unspecified() {
                    return Ok(ipv4.to_string());
                }
            }
        }
    }
    Err("No valid IP address found".into())
}

async fn update_topology(state: Arc<AppState>, lsa: &LSAMessage) -> Result<(), Box<dyn std::error::Error>> {
    let mut topology = state.topology.lock().await;
    topology.insert(
        lsa.router_ip.clone(),
        Router {
            router_ip: lsa.router_ip.clone(),
            neighbors: lsa.neighbors.clone(),
        },
    );
    Ok(())
}

async fn compute_shortest_paths(state: Arc<AppState>, source_ip: &str) -> Result<(), Box<dyn std::error::Error>> {
    let topology = state.topology.lock().await;
    let mut nodes: HashMap<String, (u32, Option<String>)> = topology
        .keys()
        .map(|id| (id.clone(), (if id == source_ip { 0 } else { u32::MAX }, None)))
        .collect();

    let mut visited = Vec::new();
    while visited.len() < topology.len() {
        let current_node = nodes
            .iter()
            .filter(|(id, _)| !visited.contains(*id))
            .min_by_key(|(_, (cost, _))| *cost)
            .map(|(id, _)| id.clone());

        if let Some(current) = current_node {
            visited.push(current.clone());
            if let Some(router) = topology.get(&current) {
                for neighbor in &router.neighbors {
                    if !neighbor.link_up {
                        continue;
                    }

                    let weight = 1000 / neighbor.capacity;
                    let new_cost = nodes[&current].0 + weight;

                    if !nodes.contains_key(&neighbor.neighbor_ip) {
                        nodes.insert(neighbor.neighbor_ip.clone(), (new_cost, Some(current.clone())));
                    } else if new_cost < nodes[&neighbor.neighbor_ip].0 {
                        nodes.insert(neighbor.neighbor_ip.clone(), (new_cost, Some(current.clone())));
                    }
                }
            }
        } else {
            break;
        }
    }

    println!("\n=== Routing Table ({}) ===", source_ip);
    for (ip, (cost, _)) in &nodes {
        if ip != source_ip {
            // Reconstruct path from source to ip
            let mut path = Vec::new();
            let mut current = ip;
            while let Some((_, Some(prev))) = nodes.get(current) {
                path.push(current.clone());
                if prev == source_ip {
                    path.push(prev.clone());
                    break;
                }
                current = prev;
            }
            path.reverse();
            let gateway = if path.len() > 1 { path[1].clone() } else { "0.0.0.0".to_string() };
            println!("To {} via {} (cost: {})", ip, gateway, cost);
            if let Err(e) = update_routing_table(ip, &gateway).await {
                log::error!("Failed to update routing table: {}", e);
            }
        }
    }
    println!("\n==========================");
    Ok(())
}

async fn update_routing_table(destination: &str, gateway: &str) -> Result<(), Box<dyn std::error::Error>> {
    let destination_ip: Ipv4Addr = destination.parse()?;
    let gateway_ip: Ipv4Addr = gateway.parse()?;

    let handle = Handle::new()?;
    
    // Calculer l'adresse réseau en appliquant un masque /24 (255.255.255.0)
    let network_addr = Ipv4Addr::from(u32::from(destination_ip) & 0xFFFFFF00);

    // Vérifie si la destination et la gateway sont sur le même réseau
    let is_direct = (u32::from(destination_ip) & 0xFFFFFF00) == (u32::from(gateway_ip) & 0xFFFFFF00);

    let route = if is_direct {
        Route::new(IpAddr::V4(network_addr), 24)
    } else {
        Route::new(IpAddr::V4(network_addr), 24)
            .with_gateway(IpAddr::V4(gateway_ip))
    };

    match handle.add(&route).await {
        Ok(_) => println!("Successfully added route to network {}/24 via {}", network_addr, gateway),
        Err(e) => {
            log::warn!("Failed to add route to network {}/24 via {}: {}", network_addr, gateway, e);
            let _ = handle.delete(&route).await;
            handle.add(&route).await?;
            println!("Successfully updated route to network {}/24 via {}", network_addr, gateway);
        }
    }

    Ok(())
}
