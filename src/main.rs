use serde::{Serialize, Deserialize};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tokio::time::{self, Duration};
use std::sync::Arc;
use net_route::{Route, Handle};
use pnet::datalink::{self, NetworkInterface};
use log::{info, warn, error, debug};
use pnet::ipnetwork::IpNetwork;
use std::fmt;
use std::error::Error as StdError;

// Constantes de configuration
const PORT: u16 = 5000;
const HELLO_INTERVAL_SEC: u64 = 20;
const LSA_INTERVAL_SEC: u64 = 15;  // Réduit de 30 à 15 secondes
const NEIGHBOR_TIMEOUT_SEC: u64 = 60;
const INITIAL_TTL: u8 = 64;
const INFINITE_METRIC: u32 = 16;

/// Représente les différentes erreurs spécifiques à notre application
#[derive(Debug)]
enum AppError {
    NetworkError(String),
    RoutingError(String),
    ConfigError(String),
    IOError(std::io::Error),
    SerializationError(serde_json::Error),
    RouteError(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::NetworkError(msg) => write!(f, "Network error: {}", msg),
            AppError::RoutingError(msg) => write!(f, "Routing error: {}", msg),
            AppError::ConfigError(msg) => write!(f, "Configuration error: {}", msg),
            AppError::IOError(err) => write!(f, "IO error: {}", err),
            AppError::SerializationError(err) => write!(f, "Serialization error: {}", err),
            AppError::RouteError(msg) => write!(f, "Route error: {}", msg),
        }
    }
}

impl StdError for AppError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            AppError::IOError(err) => Some(err),
            AppError::SerializationError(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::IOError(err)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::SerializationError(err)
    }
}

type Result<T> = std::result::Result<T, AppError>;

/// État d'une route dans la table de routage
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum RouteState {
    /// Route active avec une métrique associée
    Active(u32),
    /// Route inaccessible (empoisonnée)
    Unreachable,
}

impl fmt::Display for RouteState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RouteState::Active(metric) => write!(f, "active({})", metric),
            RouteState::Unreachable => write!(f, "unreachable"),
        }
    }
}

/// Message Hello envoyé périodiquement pour découvrir des voisins
#[derive(Debug, Serialize, Deserialize, Clone)]
struct HelloMessage {
    message_type: u8,  // Type = 1 pour Hello
    router_ip: String,
}

/// Information sur un voisin direct du routeur
#[derive(Debug, Serialize, Deserialize, Clone)]
struct Neighbor {
    neighbor_ip: String,
    link_up: bool,
    capacity: u32,  // Capacité du lien en Mbps
    last_seen: u64, // Timestamp de la dernière communication
}

/// Message LSA (Link State Advertisement) pour propager les informations de topologie
#[derive(Debug, Serialize, Deserialize, Clone)]
struct LSAMessage {
    message_type: u8,  // Type = 2 pour LSA
    router_ip: String,
    last_hop: Option<String>,            // Dernier routeur qui a relayé ce message
    originator: String,                  // Routeur qui a créé ce message
    seq_num: u32,                        // Numéro de séquence pour détecter les doublons
    neighbor_count: usize,
    neighbors: Vec<Neighbor>,
    routing_table: HashMap<String, RouteState>, // Table de routage partagée
    path: Vec<String>,                  // Liste des routeurs traversés par ce message
    ttl: u8,                            // Time to Live pour éviter les boucles
}

/// Représentation d'un routeur dans la topologie du réseau
#[derive(Debug, Clone)]
struct Router {
    router_ip: String,
    neighbors: Vec<Neighbor>,
    last_update: u64,  // Timestamp de la dernière mise à jour
}

/// État global de l'application
struct AppState {
    topology: Mutex<HashMap<String, Router>>,
    neighbors: Mutex<HashMap<String, Neighbor>>,
    routing_table: Mutex<HashMap<String, (String, RouteState)>>, // (next_hop, état)
    processed_lsa: Mutex<HashSet<(String, u32)>>, // (originator, seq_num) pour éviter de traiter plusieurs fois
    local_ip: String,
}

/// Récupère toutes les adresses de broadcast avec leurs interfaces locales associées
fn get_broadcast_addresses(port: u16) -> Vec<(String, SocketAddr)> {
    let interfaces = datalink::interfaces();
    interfaces
        .into_iter()
        .flat_map(|iface: NetworkInterface| {
            iface.ips.into_iter().filter_map(move |ip_network| {
                if let IpAddr::V4(ip) = ip_network.ip() {
                    if !ip.is_loopback() { // Exclure les adresses de loopback
                        if let IpNetwork::V4(ipv4_network) = ip_network {
                            let broadcast = ipv4_network.broadcast();
                            Some((ip.to_string(), SocketAddr::new(IpAddr::V4(broadcast), port)))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
        })
        .collect()
}

/// Met à jour la topologie du réseau avec les informations d'un message LSA
async fn update_topology(state: Arc<AppState>, lsa: &LSAMessage) -> Result<()> {
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| AppError::ConfigError(e.to_string()))?
        .as_secs();
        
    let mut topology = state.topology.lock().await;
    topology.insert(
        lsa.originator.clone(),
        Router {
            router_ip: lsa.originator.clone(),
            neighbors: lsa.neighbors.clone(),
            last_update: current_time,
        },
    );
    Ok(())
}

/// Point d'entrée principal du programme
#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn StdError>> {
    
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }

    env_logger::init();

    let router_ip = get_local_ip()?;
    info!("Router IP: {}", router_ip);

    let socket = Arc::new(UdpSocket::bind(format!("0.0.0.0:{}", PORT)).await?);
    socket.set_broadcast(true)?;

    let state = Arc::new(AppState {
        topology: Mutex::new(HashMap::new()),
        neighbors: Mutex::new(HashMap::new()),
        routing_table: Mutex::new(HashMap::new()),
        processed_lsa: Mutex::new(HashSet::new()),
        local_ip: router_ip.clone(),
    });

    // Configurer les routes directes au démarrage
    if let Err(e) = setup_direct_routes(&state).await {
        error!("Failed to setup direct routes: {}", e);
    }

    // Tâche pour envoyer périodiquement des messages Hello
    let socket_clone = Arc::clone(&socket);
    let state_clone = Arc::clone(&state);
    tokio::spawn(async move {
        let mut hello_interval = time::interval(Duration::from_secs(HELLO_INTERVAL_SEC));
        let mut lsa_interval = time::interval(Duration::from_secs(LSA_INTERVAL_SEC));
        
        loop {
            tokio::select! {
                _ = hello_interval.tick() => {
                    let broadcast_addrs = get_broadcast_addresses(PORT);
                    for (local_ip, addr) in &broadcast_addrs {
                        if let Err(e) = send_hello(&socket_clone, addr, local_ip).await {
                            error!("Failed to send hello to {}: {}", addr, e);
                        }
                    }
                }
                _ = lsa_interval.tick() => {
                    let broadcast_addrs = get_broadcast_addresses(PORT);
                    for (local_ip, addr) in &broadcast_addrs {
                        // Générer un numéro de séquence unique basé sur le timestamp
                        let seq_num = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_else(|_| Duration::from_secs(0))
                            .as_secs() as u32;
                            
                        if let Err(e) = send_lsa(&socket_clone, addr, local_ip, None, local_ip, Arc::clone(&state_clone), seq_num, vec![]).await {
                            error!("Failed to send LSA: {}", e);
                        }
                    }
                }
            }
        }
    });

    // Tâche pour surveiller l'état des voisins
    let state_clone = Arc::clone(&state);
    tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(NEIGHBOR_TIMEOUT_SEC / 2));
        
        loop {
            interval.tick().await;
            check_neighbor_timeouts(&state_clone).await;
        }
    });

    let mut buf = [0; 2048];

    // Récupère toutes les IP locales (IPv4) avec leurs interfaces
    let local_ips: HashMap<IpAddr, (String, IpNetwork)> = datalink::interfaces()
        .into_iter()
        .flat_map(|iface| {
            iface.ips.into_iter().filter_map(move |ip_network| {
                if let IpAddr::V4(ipv4) = ip_network.ip() {
                    if !ipv4.is_loopback() { // Exclude loopback addresses
                        Some((IpAddr::V4(ipv4), (ipv4.to_string(), ip_network)))
                    } else {
                        None
                    }
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
        
        debug!("Received {} bytes from {}", len, src_addr);

        // Déterminer l'IP locale de l'interface qui a reçu le paquet
        let (receiving_interface_ip, receiving_network) = match determine_receiving_interface(&src_addr.ip(), &local_ips) {
            Ok((ip, network)) => (ip, network),
            Err(e) => {
                error!("Failed to determine receiving interface: {}", e);
                continue; // Skip processing this packet
            }
        };

        match serde_json::from_slice::<serde_json::Value>(&buf[..len]) {
            Ok(json) => {
                if let Some(message_type) = json.get("message_type").and_then(|v| v.as_u64()) {
                    match message_type {
                        1 => {
                            if let Ok(hello) = serde_json::from_value::<HelloMessage>(json) {
                                info!("[RECV] HELLO from {} - {} (received on interface {})", 
                                    hello.router_ip, src_addr, receiving_interface_ip);
                                
                                update_neighbor(&state, &hello.router_ip).await;
                                
                                // Calculer l'adresse de broadcast pour l'interface qui a reçu le HELLO
                                let broadcast_addr = calculate_broadcast_for_interface(&receiving_interface_ip, &receiving_network, PORT)?;
                                
                                // Générer un numéro de séquence unique
                                let seq_num = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_else(|_| Duration::from_secs(0))
                                    .as_secs() as u32;
                                
                                // Envoyer la LSA avec l'IP de l'interface qui a reçu le HELLO
                                if let Err(e) = send_lsa(&socket, &broadcast_addr, &receiving_interface_ip, 
                                                        None, &receiving_interface_ip, Arc::clone(&state), 
                                                        seq_num, vec![receiving_interface_ip.clone()]).await {
                                    error!("Failed to send LSA after HELLO: {}", e);
                                }
                            }
                        }
                        2 => {
                            if let Ok(lsa) = serde_json::from_value::<LSAMessage>(json) {
                                info!("[RECV] LSA from {} (originator: {}, last_hop: {:?}, seq: {}) on interface {}", 
                                    src_addr, lsa.originator, lsa.last_hop, lsa.seq_num, receiving_interface_ip);
                                
                                // Vérifier si nous avons déjà traité ce LSA
                                let should_process = {
                                    let mut processed = state.processed_lsa.lock().await;
                                    let key = (lsa.originator.clone(), lsa.seq_num);
                                    if !processed.contains(&key) {
                                        processed.insert(key);
                                        true
                                    } else {
                                        false
                                    }
                                };
                                
                                if should_process && lsa.ttl > 0 {
                                    // Ne pas traiter ses propres LSA relayés
                                    if lsa.originator != receiving_interface_ip {
                                        // Vérifier si cette LSA contient notre adresse dans le chemin (pour éviter les boucles)
                                        let path_contains_us = lsa.path.contains(&receiving_interface_ip);
                                        
                                        if !path_contains_us {
                                            // Mettre à jour la table de routage
                                            if let Err(e) = update_routing_from_lsa(Arc::clone(&state), &lsa, 
                                                                                  &src_addr.ip().to_string(), &socket).await {
                                                error!("Failed to update routing from LSA: {}", e);
                                            }
                                            
                                            if let Err(e) = update_topology(Arc::clone(&state), &lsa).await {
                                                error!("Failed to update topology: {}", e);
                                            }
                                            
                                            // Retransmettre la LSA
                                            let broadcast_addr = calculate_broadcast_for_interface(&receiving_interface_ip, &receiving_network, PORT)?;
                                            
                                            // Créer un nouveau chemin incluant notre adresse
                                            let mut new_path = lsa.path.clone();
                                            new_path.push(receiving_interface_ip.clone());
                                            
                                            if let Err(e) = forward_lsa(&socket, &broadcast_addr, &receiving_interface_ip, 
                                                                      &lsa, new_path).await {
                                                error!("Failed to forward LSA: {}", e);
                                            }
                                        } else {
                                            debug!("Not forwarding LSA as it would create a loop");
                                        }
                                    } else {
                                        debug!("Not processing our own LSA");
                                    }
                                } else if !should_process {
                                    debug!("Ignoring duplicate LSA (originator: {}, seq: {})", lsa.originator, lsa.seq_num);
                                } else {
                                    debug!("LSA TTL expired, not forwarding");
                                }
                            }
                        }
                        _ => warn!("Unknown message type: {}", message_type),
                    }
                } else {
                    warn!("No message_type field in received JSON");
                }
            }
            Err(e) => {
                error!("Failed to parse JSON: {}", e);
            }
        }
    }
}

/// Met à jour l'état d'un voisin lors de la réception d'un message
async fn update_neighbor(state: &Arc<AppState>, neighbor_ip: &str) {
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs();
        
    let mut neighbors = state.neighbors.lock().await;
    
    // Mettre à jour ou créer l'entrée du voisin
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
            Neighbor {
                neighbor_ip: neighbor_ip.to_string(),
                link_up: true,
                capacity: 100, // Valeur par défaut
                last_seen: current_time,
            }
        });
}

/// Vérifie l'état des voisins et marque comme inactifs ceux qui n'ont pas été vus récemment
async fn check_neighbor_timeouts(state: &Arc<AppState>) {
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs();
        
    let mut neighbors = state.neighbors.lock().await;
    let mut changed = false;
    
    // Vérifier tous les voisins
    for (ip, neighbor) in neighbors.iter_mut() {
        if neighbor.link_up && current_time - neighbor.last_seen > NEIGHBOR_TIMEOUT_SEC {
            warn!("Neighbor {} is DOWN (timeout)", ip);
            neighbor.link_up = false;
            changed = true;
        }
    }
    
    drop(neighbors);
    
    // Si des voisins ont été marqués comme inactifs, mettre à jour les routes
    if changed {
        // Envoyer une LSA pour informer du changement
        let broadcast_addrs = get_broadcast_addresses(PORT);
        let socket = UdpSocket::bind("0.0.0.0:0").await.unwrap_or_else(|_| panic!("Failed to create socket"));
        socket.set_broadcast(true).unwrap_or_else(|_| panic!("Failed to set broadcast"));
        
        for (local_ip, addr) in &broadcast_addrs {
            let seq_num = current_time as u32;
            if let Err(e) = send_lsa(&socket, addr, local_ip, None, local_ip, Arc::clone(&state), seq_num, vec![]).await {
                error!("Failed to send LSA after neighbor timeout: {}", e);
            }
        }
    }
}

/// Fonction pour déterminer l'IP de l'interface qui a reçu le paquet
fn determine_receiving_interface(
    sender_ip: &IpAddr,
    local_ips: &HashMap<IpAddr, (String, IpNetwork)>,
) -> Result<(String, IpNetwork)> {
    if let IpAddr::V4(sender_ipv4) = sender_ip {
        // Chercher l'interface locale qui est sur le même réseau que l'expéditeur
        for (local_ip, (local_ip_str, ip_network)) in local_ips {
            if let IpNetwork::V4(ipv4_network) = ip_network {
                if ipv4_network.contains(*sender_ipv4) {
                    return Ok((local_ip_str.clone(), ip_network.clone()));
                }
            }
        }
    }

    // Si aucune interface correspondante n'est trouvée, utiliser la première IP locale non-loopback
    for (local_ip, (local_ip_str, ip_network)) in local_ips {
        if let IpAddr::V4(ipv4) = local_ip {
            if !ipv4.is_loopback() && !ipv4.is_unspecified() {
                return Ok((local_ip_str.clone(), ip_network.clone()));
            }
        }
    }

    Err(AppError::NetworkError("No valid receiving interface found".to_string()))
}

/// Fonction pour calculer l'adresse de broadcast pour une interface donnée
fn calculate_broadcast_for_interface(interface_ip: &str, ip_network: &IpNetwork, port: u16) -> Result<SocketAddr> {
    if let IpNetwork::V4(ipv4_network) = ip_network {
        let broadcast_addr = ipv4_network.broadcast();
        Ok(SocketAddr::new(IpAddr::V4(broadcast_addr), port))
    } else {
        Err(AppError::NetworkError("Invalid IPv4 network".to_string()))
    }
}

/// Envoie un message Hello pour découvrir des voisins
async fn send_hello(socket: &UdpSocket, addr: &SocketAddr, router_ip: &str) -> Result<()> {
    let message = HelloMessage {
        message_type: 1,
        router_ip: router_ip.to_string(),
    };
    let serialized = serde_json::to_vec(&message).map_err(AppError::from)?;
    socket.send_to(&serialized, addr).await.map_err(AppError::from)?;
    info!("[SEND] HELLO from {} to {}", router_ip, addr);
    Ok(())
}

/// Envoie un message LSA (Link State Advertisement)
async fn send_lsa(
    socket: &UdpSocket, 
    addr: &SocketAddr, 
    router_ip: &str, 
    last_hop: Option<&str>,
    originator: &str,
    state: Arc<AppState>,
    seq_num: u32,
    path: Vec<String>
) -> Result<()> {
    let neighbors_guard = state.neighbors.lock().await;
    let neighbors_vec = neighbors_guard.values().cloned().collect::<Vec<_>>();
    drop(neighbors_guard);

    let routing_table_guard = state.routing_table.lock().await;
    let mut route_states = HashMap::new();
    
    // Inclure toutes les entrées de la table de routage dans le LSA
    for (dest, (next_hop, state)) in routing_table_guard.iter() {
        route_states.insert(dest.clone(), state.clone());
        
        // Si la métrique est 0, c'est un réseau directement connecté
        // On veut s'assurer qu'il est propagé avec priorité
        if let RouteState::Active(0) = state {
            info!("Advertising directly connected network: {}", dest);
        }
    }
    drop(routing_table_guard);

    // Créer et envoyer le message LSA...
    let message = LSAMessage {
        message_type: 2,
        router_ip: router_ip.to_string(),
        last_hop: last_hop.map(|s| s.to_string()),
        originator: originator.to_string(),
        seq_num,
        neighbor_count: neighbors_vec.len(),
        neighbors: neighbors_vec,
        routing_table: route_states,
        path,
        ttl: INITIAL_TTL,
    };

    let serialized = serde_json::to_vec(&message).map_err(AppError::from)?;
    socket.send_to(&serialized, addr).await.map_err(AppError::from)?;
    info!("[SEND] LSA from {} (originator: {}, seq: {}) to {}", 
          router_ip, originator, seq_num, addr);
    Ok(())
}

/// Transfère un message LSA vers d'autres routeurs
async fn forward_lsa(
    socket: &UdpSocket,
    addr: &SocketAddr,
    router_ip: &str,
    original_lsa: &LSAMessage,
    path: Vec<String>,
) -> Result<()> {
    if original_lsa.ttl <= 1 {
        return Ok(());
    }
    
    let message = LSAMessage {
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

    let serialized = serde_json::to_vec(&message).map_err(AppError::from)?;
    socket.send_to(&serialized, addr).await.map_err(AppError::from)?;
    info!("[FORWARD] LSA from {} (originator: {}, seq: {}) to {}", 
          router_ip, original_lsa.originator, original_lsa.seq_num, addr);
    Ok(())
}

/// Récupère la première adresse IP non-loopback
fn get_local_ip() -> Result<String> {
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
    Err(AppError::ConfigError("No valid IP address found".to_string()))
}

/// Convertit une IP en adresse réseau (CIDR)
fn ip_to_network(ip: &str, prefix: u8) -> Result<String> {
    let ip: Ipv4Addr = ip.parse()
        .map_err(|e| AppError::RouteError(format!("Invalid IP {}: {}", ip, e)))?;
    
    // Calculer le masque et l'adresse réseau
    let mask = (!0u32) << (32 - prefix);
    let network = u32::from(ip) & mask;
    
    // Formater l'adresse réseau avec le préfixe
    Ok(format!("{}.{}.{}.{}/{}",
        (network >> 24) & 0xFF,
        (network >> 16) & 0xFF,
        (network >> 8) & 0xFF,
        network & 0xFF,
        prefix))
}

/// Met à jour la table de routage en fonction des informations d'un message LSA
async fn update_routing_from_lsa(
    state: Arc<AppState>,
    lsa: &LSAMessage,
    sender_ip: &str,
    socket: &UdpSocket
) -> Result<()> {
    let mut routing_table = state.routing_table.lock().await;
    let next_hop = sender_ip.to_string();
    let prefix = 24; // Préfixe standard pour un réseau local
    
    // Récupérer la liste des réseaux directement connectés (routes à préserver)
    let direct_networks: HashSet<String> = routing_table.iter()
        .filter_map(|(network, (_, state))| {
            if let RouteState::Active(0) = state {
                Some(network.clone())
            } else {
                None
            }
        })
        .collect();
    
    // Récupérer la liste des voisins directs
    let neighbors_guard = state.neighbors.lock().await;
    let direct_neighbors: HashSet<String> = neighbors_guard.iter()
        .filter(|(_, n)| n.link_up)
        .map(|(ip, _)| {
            ip_to_network(ip, prefix).unwrap_or_else(|_| ip.clone())
        })
        .collect();
    drop(neighbors_guard);
    
    // Suite du traitement du LSA...
    
    // Traiter la table de routage du LSA
    for (dest, route_state) in &lsa.routing_table {
        // Vérifier si c'est déjà une adresse réseau (avec un '/')
        let dest_network = if dest.contains('/') {
            dest.clone()
        } else if let Ok(network) = ip_to_network(dest, prefix) {
            network
        } else {
            continue; // Ignorer les destinations invalides
        };
        
        // Ne pas modifier nos propres réseaux directement connectés
        if direct_networks.contains(&dest_network) {
            debug!("Skipping update for directly connected network: {}", dest_network);
            continue;
        }
        
        match route_state {
            RouteState::Active(metric) => {
                let existing_entry = routing_table.get(&dest_network);
                // Préserver la métrique originale pour les réseaux directement connectés
                let new_metric = if *metric == 0 {
                    // Si c'est un réseau directement connecté pour l'émetteur LSA, ajouter 1
                    1
                } else {
                    // Sinon, ajouter la métrique normale
                    metric + 1
                };
                
                let should_update = match existing_entry {
                    Some((_, RouteState::Active(current_metric))) => {
                        // Si la route existante est directe (métrique=0), ne pas la remplacer
                        if *current_metric == 0 {
                            false
                        } else {
                            new_metric < *current_metric
                        }
                    },
                    Some((_, RouteState::Unreachable)) => true,
                    None => true,
                };
                
                if should_update {
                    // Ne pas mettre à jour les routes vers nos voisins directs (elles sont déjà optimales)
                    if !direct_neighbors.contains(&dest_network) {
                        // Mettre à jour la route
                        routing_table.insert(dest_network.clone(), (next_hop.clone(), RouteState::Active(new_metric)));
                        info!("Learned route from LSA: {} -> next_hop: {} (metric: {})", 
                         dest_network, next_hop, new_metric);
                        
                        if let Err(e) = update_routing_table_safe(&dest_network, &next_hop, prefix).await {
                            warn!("Could not update system routing table for {}: {}", dest_network, e);
                        }
                    }
                }
            },
            // Traitement des routes unreachable...
            RouteState::Unreachable => {
                // Gérer les routes inaccessibles (même code qu'avant)
                if let Some((current_next_hop, _)) = routing_table.get(&dest_network) {
                    if current_next_hop == &next_hop {
                        routing_table.insert(dest_network.clone(), (next_hop.clone(), RouteState::Unreachable));
                        info!("Route marked as unreachable: {}", dest_network);
                    }
                }
            }
        }
    }
    
    Ok(())
}

/// Envoie une annonce de route empoisonnée
async fn send_poisoned_route(
    socket: &UdpSocket,
    addr: &SocketAddr,
    router_ip: &str,
    poisoned_route: &str,
    seq_num: u32,
    path: Vec<String>
) -> Result<()> {
    let mut routing_table = HashMap::new();
    routing_table.insert(poisoned_route.to_string(), RouteState::Unreachable);
    
    let message = LSAMessage {
        message_type: 2,
        router_ip: router_ip.to_string(),
        last_hop: None,
        originator: router_ip.to_string(),
        seq_num,
        neighbor_count: 0,
        neighbors: Vec::new(),
        routing_table,
        path,
        ttl: INITIAL_TTL,
    };
    
    let serialized = serde_json::to_vec(&message).map_err(AppError::from)?;
    socket.send_to(&serialized, addr).await.map_err(AppError::from)?;
    info!("[SEND] POISON ROUTE for {} from {} to {}", poisoned_route, router_ip, addr);
    Ok(())
}

/// Version adaptée pour Alpine Linux de la mise à jour de la table de routage
async fn update_routing_table_safe(destination: &str, gateway: &str, prefix: u8) -> Result<()> {
    // Vérifier si la destination contient déjà un préfixe (format CIDR)
    let (destination_ip, actual_prefix) = if destination.contains('/') {
        let parts: Vec<&str> = destination.split('/').collect();
        if parts.len() == 2 {
            let ip = parts[0].parse::<Ipv4Addr>()
                .map_err(|e| AppError::RouteError(format!("Invalid destination IP {}: {}", parts[0], e)))?;
            let net_prefix = parts[1].parse::<u8>()
                .map_err(|e| AppError::RouteError(format!("Invalid prefix {}: {}", parts[1], e)))?;
            (ip, net_prefix)
        } else {
            return Err(AppError::RouteError(format!("Invalid CIDR format: {}", destination)));
        }
    } else {
        // C'est une adresse IP simple, utiliser le préfixe fourni
        let ip = destination.parse::<Ipv4Addr>()
            .map_err(|e| AppError::RouteError(format!("Invalid destination IP {}: {}", destination, e)))?;
        (ip, prefix)
    };

    let gateway_ip: Ipv4Addr = gateway.parse()
        .map_err(|e| AppError::RouteError(format!("Invalid gateway IP {}: {}", gateway, e)))?;

    // Éviter les adresses invalides
    if destination_ip.is_loopback() || destination_ip.is_unspecified() || 
       gateway_ip.is_loopback() || gateway_ip.is_unspecified() {
        debug!("Skipping route to invalid address: {} via {}", destination, gateway);
        return Ok(());
    }
    
    // Déterminer si c'est une route directe (même sous-réseau)
    let dest_network = format!("{}.{}.{}.0", 
        destination_ip.octets()[0], 
        destination_ip.octets()[1], 
        destination_ip.octets()[2]);
    let gateway_network = format!("{}.{}.{}.0", 
        gateway_ip.octets()[0], 
        gateway_ip.octets()[1], 
        gateway_ip.octets()[2]);
    
    // Pour les réseaux directement connectés, ne pas modifier la route
    if dest_network == gateway_network {
        debug!("Skipping route update for directly connected network: {}/{}", destination_ip, actual_prefix);
        return Ok(());
    }

    // Sur Alpine, utiliser les commandes système directement au lieu de la bibliothèque net_route
    // qui peut avoir des problèmes de compatibilité
    use tokio::process::Command;
    
    // D'abord tenter de supprimer toute route existante pour éviter les conflits
    let delete_output = Command::new("ip")
        .args(&["route", "del", &format!("{}/{}", destination_ip, actual_prefix)])
        .output()
        .await;
    
    // Ignorer les erreurs de suppression, elles sont attendues si la route n'existe pas
    
    // Ajouter la nouvelle route
    let add_output = Command::new("ip")
        .args(&["route", "add", &format!("{}/{}", destination_ip, actual_prefix), 
              "via", &gateway_ip.to_string()])
        .output()
        .await;
    
    match add_output {
        Ok(output) => {
            if output.status.success() {
                info!("Added route to {}/{} via {}", destination_ip, actual_prefix, gateway_ip);
                Ok(())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                // Si l'erreur indique que la route existe déjà, ce n'est pas une erreur grave
                if stderr.contains("File exists") {
                    debug!("Route already exists: {}/{} via {}", destination_ip, actual_prefix, gateway_ip);
                    Ok(())
                } else {
                    warn!("Failed to add route: {}", stderr);
                    Err(AppError::RouteError(format!("Failed to add route: {}", stderr)))
                }
            }
        },
        Err(e) => {
            warn!("Failed to execute route command: {}", e);
            Err(AppError::RouteError(format!("Command execution failed: {}", e)))
        }
    }
}

/// Configure les routes directes au démarrage
async fn setup_direct_routes(state: &Arc<AppState>) -> Result<()> {
    let interfaces = datalink::interfaces();
    
    for interface in interfaces {
        if interface.is_loopback() || !interface.is_up() {
            continue;
        }
        
        for ip_network in &interface.ips {
            if let IpNetwork::V4(ipv4_network) = ip_network {
                if ipv4_network.ip().is_loopback() || ipv4_network.ip().is_unspecified() {
                    continue;
                }
                
                let network_str = format!("{}", ipv4_network);
                let ip_str = ipv4_network.ip().to_string();
                
                // Ajouter à la table de routage interne avec métrique 0 pour indiquer connexion directe
                let mut routing_table = state.routing_table.lock().await;
                routing_table.insert(network_str.clone(), (ip_str.clone(), RouteState::Active(0)));
                drop(routing_table);
                
                info!("Added direct route to network: {} (directly connected)", network_str);
            }
        }
    }
    
    Ok(())
}
