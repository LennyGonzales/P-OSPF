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
    last_hop: Option<String>, // Le dernier routeur par lequel le message est passé
    originator: String,       // Le routeur qui émet originalement la LSA
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
    routing_table: Mutex<HashMap<String, String>>, // destination -> next_hop
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

    let router_ip = get_local_ip()?;
    println!("Router IP: {}", router_ip);

    let socket = Arc::new(UdpSocket::bind("0.0.0.0:5000").await?);
    socket.set_broadcast(true)?;

    let state = Arc::new(AppState {
        topology: Mutex::new(HashMap::new()),
        neighbors: Mutex::new(HashMap::new()),
        routing_table: Mutex::new(HashMap::new()),
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

    // Récupère toutes les IP locales (IPv4) avec leurs interfaces
    let local_ips: HashMap<IpAddr, String> = datalink::interfaces()
        .into_iter()
        .flat_map(|iface| {
            iface.ips.into_iter().filter_map(move |ip_network| {
                if let IpAddr::V4(ipv4) = ip_network.ip() {
                    Some((IpAddr::V4(ipv4), ipv4.to_string()))
                } else {
                    None
                }
            })
        })
        .collect();

    loop {
        let (len, src_addr) = socket.recv_from(&mut buf).await?;
        
        // Ignore les paquets venant d'une IP locale
        if local_ips.contains_key(&src_addr.ip()) {
            continue;
        }
        
        println!("Received {} bytes from {}", len, src_addr);

        // Déterminer l'IP locale de l'interface qui a reçu le paquet
        let receiving_interface_ip = determine_receiving_interface(&src_addr.ip(), &local_ips)?;

        match serde_json::from_slice::<serde_json::Value>(&buf[..len]) {
            Ok(json) => {
                if let Some(message_type) = json.get("message_type").and_then(|v| v.as_u64()) {
                    match message_type {
                        1 => {
                            println!("IN [RECV] HELLO");
                            if let Ok(hello) = serde_json::from_value::<HelloMessage>(json) {
                                println!("[RECV] HELLO from {} - {} (received on interface {})", 
                                    hello.router_ip, src_addr, receiving_interface_ip);
                                
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

                                // Calculer l'adresse de broadcast pour l'interface qui a reçu le HELLO
                                let broadcast_addr = calculate_broadcast_for_interface(&receiving_interface_ip, 5000)?;
                                
                                // Envoyer la LSA avec l'IP de l'interface qui a reçu le HELLO
                                if let Err(e) = send_lsa(&socket, &broadcast_addr, &receiving_interface_ip, None, &receiving_interface_ip, state.clone()).await {
                                    log::error!("Failed to send LSA: {}", e);
                                }
                            }
                        }
                        2 => {
                            if let Ok(lsa) = serde_json::from_value::<LSAMessage>(json) {
                                println!("[RECV] LSA from {} (originator: {}, last_hop: {:?}) on interface {}", 
                                    src_addr, lsa.originator, lsa.last_hop, receiving_interface_ip);
                                
                                // Mettre à jour la table de routage en fonction des informations LSA
                                if let Err(e) = update_routing_from_lsa(state.clone(), &lsa, &src_addr.ip().to_string()).await {
                                    log::error!("Failed to update routing from LSA: {}", e);
                                }
                                
                                if let Err(e) = update_topology(state.clone(), &lsa).await {
                                    log::error!("Failed to update topology: {}", e);
                                }
                                
                                // Retransmettre la LSA avec nous comme last_hop si ce n'est pas notre LSA
                                if lsa.originator != receiving_interface_ip {
                                    let broadcast_addr = calculate_broadcast_for_interface(&receiving_interface_ip, 5000)?;
                                    if let Err(e) = forward_lsa(&socket, &broadcast_addr, &receiving_interface_ip, &lsa, state.clone()).await {
                                        log::error!("Failed to forward LSA: {}", e);
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

// Fonction pour déterminer l'IP de l'interface qui a reçu le paquet
fn determine_receiving_interface(sender_ip: &IpAddr, local_ips: &HashMap<IpAddr, String>) -> Result<String, Box<dyn std::error::Error>> {
    if let IpAddr::V4(sender_ipv4) = sender_ip {
        let sender_u32 = u32::from(*sender_ipv4);
        
        // Chercher l'interface locale qui est sur le même réseau que l'expéditeur
        for (local_ip, local_ip_str) in local_ips {
            if let IpAddr::V4(local_ipv4) = local_ip {
                let local_u32 = u32::from(*local_ipv4);
                
                // Vérifier si ils sont sur le même réseau /24
                if (sender_u32 & 0xFFFFFF00) == (local_u32 & 0xFFFFFF00) {
                    return Ok(local_ip_str.clone());
                }
            }
        }
    }
    
    // Si aucune interface correspondante n'est trouvée, utiliser la première IP locale non-loopback
    for (local_ip, local_ip_str) in local_ips {
        if let IpAddr::V4(ipv4) = local_ip {
            if !ipv4.is_loopback() && !ipv4.is_unspecified() {
                return Ok(local_ip_str.clone());
            }
        }
    }
    
    Err("No valid receiving interface found".into())
}

// Fonction pour calculer l'adresse de broadcast pour une interface donnée
fn calculate_broadcast_for_interface(interface_ip: &str, port: u16) -> Result<SocketAddr, Box<dyn std::error::Error>> {
    let ip: Ipv4Addr = interface_ip.parse()?;
    let ip_u32 = u32::from(ip);
    
    // Supposer un masque /24 (255.255.255.0)
    let mask = 0xFFFFFF00;
    let broadcast_u32 = ip_u32 | !mask;
    let broadcast_ip = Ipv4Addr::from(broadcast_u32);
    
    Ok(SocketAddr::new(IpAddr::V4(broadcast_ip), port))
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

async fn send_lsa(
    socket: &UdpSocket, 
    addr: &SocketAddr, 
    router_ip: &str, 
    last_hop: Option<&str>,
    originator: &str,
    state: Arc<AppState>
) -> Result<(), Box<dyn std::error::Error>> {
    let neighbors = state.neighbors.lock().await;
    let neighbors_vec = neighbors.values().cloned().collect::<Vec<_>>();

    let message = LSAMessage {
        message_type: 2,
        router_ip: router_ip.to_string(),
        last_hop: last_hop.map(|s| s.to_string()),
        originator: originator.to_string(),
        neighbor_count: neighbors_vec.len(),
        neighbors: neighbors_vec,
    };

    let serialized = serde_json::to_vec(&message)?;
    socket.send_to(&serialized, addr).await?;
    println!("[SEND] LSA from {} (originator: {}, last_hop: {:?}) to {}", router_ip, originator, last_hop, addr);
    Ok(())
}

async fn forward_lsa(
    socket: &UdpSocket,
    addr: &SocketAddr,
    router_ip: &str,
    original_lsa: &LSAMessage,
    state: Arc<AppState>
) -> Result<(), Box<dyn std::error::Error>> {
    let message = LSAMessage {
        message_type: 2,
        router_ip: router_ip.to_string(),
        last_hop: Some(router_ip.to_string()),
        originator: original_lsa.originator.clone(),
        neighbor_count: original_lsa.neighbor_count,
        neighbors: original_lsa.neighbors.clone(),
    };

    let serialized = serde_json::to_vec(&message)?;
    socket.send_to(&serialized, addr).await?;
    println!("[FORWARD] LSA from {} (originator: {}) to {}", router_ip, original_lsa.originator, addr);
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

async fn update_routing_from_lsa(
    state: Arc<AppState>,
    lsa: &LSAMessage,
    sender_ip: &str
) -> Result<(), Box<dyn std::error::Error>> {
    let mut routing_table = state.routing_table.lock().await;
    
    // Si c'est une LSA originale (pas de last_hop), le next_hop est l'expéditeur
    let next_hop = if lsa.last_hop.is_none() {
        sender_ip.to_string()
    } else {
        // Si la LSA a un last_hop, utiliser l'expéditeur comme next_hop
        sender_ip.to_string()
    };
    
    // Mettre à jour la route vers l'originateur de la LSA
    if lsa.originator != sender_ip {
        routing_table.insert(lsa.originator.clone(), next_hop.clone());
        println!("Updated route: {} -> next_hop: {}", lsa.originator, next_hop);
        
        // Mettre à jour la table de routage système
        if let Err(e) = update_routing_table(&lsa.originator, &next_hop).await {
            log::error!("Failed to update system routing table: {}", e);
        }
    }
    
    // Mettre à jour les routes vers tous les voisins mentionnés dans la LSA
    for neighbor in &lsa.neighbors {
        if neighbor.link_up && !routing_table.contains_key(&neighbor.neighbor_ip) {
            routing_table.insert(neighbor.neighbor_ip.clone(), next_hop.clone());
            println!("Updated route: {} -> next_hop: {}", neighbor.neighbor_ip, next_hop);
            
            // Mettre à jour la table de routage système
            if let Err(e) = update_routing_table(&neighbor.neighbor_ip, &next_hop).await {
                log::error!("Failed to update system routing table: {}", e);
            }
        }
    }
    
    Ok(())
}

async fn update_topology(state: Arc<AppState>, lsa: &LSAMessage) -> Result<(), Box<dyn std::error::Error>> {
    let mut topology = state.topology.lock().await;
    topology.insert(
        lsa.originator.clone(),
        Router {
            router_ip: lsa.originator.clone(),
            neighbors: lsa.neighbors.clone(),
        },
    );
    Ok(())
}

async fn compute_shortest_paths(state: Arc<AppState>, source_ip: &str) -> Result<(), Box<dyn std::error::Error>> {
    let topology = state.topology.lock().await;
    let routing_table = state.routing_table.lock().await;
    
    println!("\n=== Current Routing Table ({}) ===", source_ip);
    for (destination, next_hop) in routing_table.iter() {
        println!("To {} via {}", destination, next_hop);
    }
    println!("==================================\n");
    
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
