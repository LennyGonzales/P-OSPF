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
const LSA_INTERVAL_SEC: u64 = 30;
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
    router_id: String,  // Router ID stable et unique
    interfaces: HashMap<String, IpNetwork>,  // Toutes les interfaces du routeur
}

/// Récupère toutes les adresses de broadcast avec leurs interfaces locales associées
fn get_broadcast_addresses(port: u16) -> Vec<(String, SocketAddr)> {
    let interfaces = datalink::interfaces();
    interfaces
        .into_iter()
        .flat_map(|iface: NetworkInterface| {
            iface.ips.into_iter().filter_map(move |ip_network| {
                if let IpNetwork::V4(ipv4_network) = ip_network {
                    let ip = ipv4_network.ip();
                    if !ip.is_loopback() {
                        // Calculer l'adresse de broadcast pour ce réseau
                        let broadcast_addr = ipv4_network.broadcast();
                        Some((
                            ip.to_string(),
                            SocketAddr::new(IpAddr::V4(broadcast_addr), port),
                        ))
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
        .unwrap_or_else(|_| Duration::from_secs(0))
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

/// Détermination du Router ID au démarrage
fn determine_router_id() -> Result<String> {
    // Stratégie: prendre la plus haute adresse IP parmi toutes les interfaces
    let mut highest_ip = Ipv4Addr::new(0, 0, 0, 0);
    let interfaces = datalink::interfaces();
    
    // Chercher d'abord une interface loopback
    for iface in &interfaces {
        for ip_network in &iface.ips {
            if let IpNetwork::V4(ipv4_net) = ip_network {
                let ip = ipv4_net.ip();
                if ip.is_loopback() {
                    return Ok(ip.to_string());
                }
            }
        }
    }
    
    // Sinon, prendre l'adresse IP la plus élevée
    for iface in interfaces {
        for ip_network in iface.ips {
            if let IpNetwork::V4(ipv4_net) = ip_network {
                let ip = ipv4_net.ip();
                if !ip.is_loopback() && !ip.is_unspecified() && ip > highest_ip {
                    highest_ip = ip;
                }
            }
        }
    }
    
    if highest_ip == Ipv4Addr::new(0, 0, 0, 0) {
        return Err(AppError::ConfigError("No valid interfaces found for Router ID".to_string()));
    }
    
    Ok(highest_ip.to_string())
}

/// Point d'entrée principal du programme
#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn StdError>> {
    // Configuration du logger
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }
    env_logger::init();
    
    // Vérifier les privilèges pour la manipulation de routes
    if !cfg!(windows) && unsafe { libc::geteuid() } != 0 {
        warn!("Not running as root/administrator - route manipulation may fail");
    }
    
    let router_ip = get_local_ip()?;
    info!("Router IP: {}", router_ip);

    let socket = Arc::new(UdpSocket::bind(format!("0.0.0.0:{}", PORT)).await?);
    socket.set_broadcast(true)?;

    let router_id = determine_router_id()?;
    info!("Router ID: {}", router_id);
    
    // Collecter toutes les interfaces du routeur
    let mut interfaces = HashMap::new();
    for iface in datalink::interfaces() {
        for ip_network in iface.ips {
            if let IpNetwork::V4(ipv4_net) = ip_network {
                let ip = ipv4_net.ip();
                if !ip.is_loopback() && !ip.is_unspecified() {
                    interfaces.insert(ip.to_string(), ip_network);
                }
            }
        }
    }
    
    let state = Arc::new(AppState {
        topology: Mutex::new(HashMap::new()),
        neighbors: Mutex::new(HashMap::new()),
        routing_table: Mutex::new(HashMap::new()),
        processed_lsa: Mutex::new(HashSet::new()),
        local_ip: router_ip.clone(),
        router_id: router_id.clone(),
        interfaces,
    });

    // Ajout des routes directement connectées à la table de routage
    {
        let mut routing_table = state.routing_table.lock().await;
        for iface in datalink::interfaces() {
            for ip_network in iface.ips {
                if let IpNetwork::V4(ipv4_network) = ip_network {
                    let ip = ipv4_network.ip();
                    if !ip.is_loopback() && !ip.is_unspecified() {
                        // Représenter le réseau sous forme CIDR
                        let network_str = ipv4_network.to_string();
                        // Next hop = 0.0.0.0 pour indiquer route connectée
                        routing_table.insert(network_str, ("0.0.0.0".to_string(), RouteState::Active(0)));
                    }
                }
            }
        }
    }

    // Tâche pour envoyer périodiquement des messages Hello et LSA
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
                            error!("Failed to send Hello: {}", e);
                        }
                    }
                }
                _ = lsa_interval.tick() => {
                    let broadcast_addrs = get_broadcast_addresses(PORT);
                    for (local_ip, addr) in &broadcast_addrs {
                        let seq_num = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
                            Ok(d) => d.as_secs() as u32,
                            Err(_) => 0,
                        };
                        if let Err(e) = send_lsa(&socket_clone, addr, local_ip, None, &state_clone.router_id, Arc::clone(&state_clone), seq_num, vec![]).await {
                            error!("Failed to send LSA: {}", e);
                        }
                    }
                }
            }
        }
    });

    // Tâche pour surveiller l'état des voisins
    let state_monitor = Arc::clone(&state);
    tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(NEIGHBOR_TIMEOUT_SEC / 2));
        
        loop {
            interval.tick().await;
            check_neighbor_timeouts(&state_monitor).await;
        }
    });

    let mut buf = [0; 2048];

    // Récupère toutes les IP locales (IPv4) avec leurs interfaces
    let local_ips: HashMap<IpAddr, (String, IpNetwork)> = datalink::interfaces()
        .into_iter()
        .flat_map(|iface| {
            iface.ips.into_iter().filter_map(move |ip_network| {
                if let IpAddr::V4(ipv4) = ip_network.ip() {
                    if !ipv4.is_loopback() {
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
                continue;
            }
        };

        match serde_json::from_slice::<serde_json::Value>(&buf[..len]) {
            Ok(json) => {
                if let Some(message_type) = json.get("message_type").and_then(|v| v.as_u64()) {
                    if message_type == 1 {
                        // Message Hello
                        if let Ok(hello_msg) = serde_json::from_value::<HelloMessage>(json.clone()) {
                            // Mettre à jour l'état du voisin
                            update_neighbor(&state, &hello_msg.router_ip).await;
                        }
                    } else if message_type == 2 {
                        // Message LSA
                        if let Ok(lsa_msg) = serde_json::from_value::<LSAMessage>(json.clone()) {
                            if let Err(e) = process_lsa(&state, &lsa_msg, &receiving_interface_ip, &socket).await {
                                warn!("Failed to process LSA: {}", e);
                            }
                        }
                    }
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
    let current_time = match std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH) {
            Ok(duration) => duration.as_secs(),
            Err(e) => {
                error!("System time error: {}", e);
                return;
            }
        };
        
    let mut neighbors = state.neighbors.lock().await;
    let mut changed = false;
    
    for (ip, neighbor) in neighbors.iter_mut() {
        if neighbor.link_up && current_time - neighbor.last_seen > NEIGHBOR_TIMEOUT_SEC {
            warn!("Neighbor {} is DOWN (timeout)", ip);
            neighbor.link_up = false;
            changed = true;
        }
    }
    
    drop(neighbors);
    
    // Si des voisins ont été marqués comme inactifs, émettre une LSA et relancer SPF
    if changed {
        let broadcast_addrs = get_broadcast_addresses(PORT);
        let socket = match UdpSocket::bind("0.0.0.0:0").await {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to create socket for timeout notification: {}", e);
                return;
            }
        };
        
        if let Err(e) = socket.set_broadcast(true) {
            error!("Failed to set broadcast permission: {}", e);
            return;
        }
        
        for (_, addr) in &broadcast_addrs {
            let seq_num = current_time as u32;
            if let Err(e) = send_lsa(&socket, addr, &state.router_id, None, &state.router_id, Arc::clone(state), seq_num, vec![]).await {
                error!("Failed to send LSA after neighbor timeout: {}", e);
            }
        }
        
        if let Err(e) = run_spf_algorithm(state).await {
            error!("Failed to run SPF after neighbor timeout: {}", e);
        }
    }
}

/// Permet de traiter un message LSA
async fn process_lsa(
    state: &Arc<AppState>,
    lsa: &LSAMessage,
    sender_ip: &str,
    socket: &UdpSocket
) -> Result<()> {
    // Vérifier si on a déjà traité ce LSA
    {
        let mut processed = state.processed_lsa.lock().await;
        if processed.contains(&(lsa.originator.clone(), lsa.seq_num)) {
            debug!("LSA {} (seq {}) from {} already processed", lsa.originator, lsa.seq_num, sender_ip);
            return Ok(());
        }
        processed.insert((lsa.originator.clone(), lsa.seq_num));
    }

    update_topology(Arc::clone(state), lsa).await?;
    update_routing_from_lsa(Arc::clone(state), lsa, sender_ip, socket).await?;

    // Transférer ce LSA vers d'autres routeurs
    let broadcast_addrs = get_broadcast_addresses(PORT);
    let mut path = lsa.path.clone();
    path.push(state.router_id.clone());
    
    for (local_ip, addr) in &broadcast_addrs {
        // Éviter d'envoyer à l'interface source
        if local_ip == sender_ip {
            continue;
        }
        forward_lsa(
            socket,
            addr,
            &state.router_id,
            lsa,
            path.clone()
        ).await?;
    }
    
    Ok(())
}

/// Fonction pour déterminer l'IP de l'interface qui a reçu le paquet
fn determine_receiving_interface(
    sender_ip: &IpAddr,
    local_ips: &HashMap<IpAddr, (String, IpNetwork)>,
) -> Result<(String, IpNetwork)> {
    if let IpAddr::V4(sender_ipv4) = sender_ip {
        for (_, (local_ip_str, ip_network)) in local_ips {
            if let IpNetwork::V4(ipv4_network) = ip_network {
                if ipv4_network.contains(*sender_ipv4) {
                    return Ok((local_ip_str.clone(), ip_network.clone()));
                }
            }
        }
    } else if let IpAddr::V6(sender_ipv6) = sender_ip {
        for (_, (local_ip_str, ip_network)) in local_ips {
            if let IpNetwork::V6(ipv6_network) = ip_network {
                if ipv6_network.contains(*sender_ipv6) {
                    return Ok((local_ip_str.clone(), ip_network.clone()));
                }
            }
        }
    }

    // Si aucune correspondance directe n'est trouvée, utiliser l'interface par défaut
    for (_, (local_ip_str, ip_network)) in local_ips {
        match ip_network.ip() {
            IpAddr::V4(ipv4) if !ipv4.is_loopback() && !ipv4.is_unspecified() => {
                warn!("No matching interface for {}, using {} as default", sender_ip, local_ip_str);
                return Ok((local_ip_str.clone(), ip_network.clone()));
            }
            IpAddr::V6(ipv6) if !ipv6.is_loopback() && !ipv6.is_unspecified() => {
                warn!("No matching interface for {}, using {} as default", sender_ip, local_ip_str);
                return Ok((local_ip_str.clone(), ip_network.clone()));
            }
            _ => {}
        }
    }
    Err(AppError::NetworkError("No valid receiving interface found".to_string()))
}

/// Fonction pour calculer l'adresse de broadcast pour une interface (test unitaire)
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
    for (dest, (_, state_val)) in routing_table_guard.iter() {
        route_states.insert(dest.clone(), state_val.clone());
    }
    drop(routing_table_guard);

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

/// Version améliorée pour obtenir le réseau associé à une adresse IP
fn get_network_for_ip_improved(ip: &IpAddr) -> Option<(IpAddr, IpNetwork)> {
    if let IpAddr::V4(ipv4) = ip {
        // Recherche directe dans les interfaces locales
        for interface in datalink::interfaces() {
            for ip_network in &interface.ips {
                if let IpNetwork::V4(network) = ip_network {
                    if network.contains(*ipv4) {
                        return Some((*ip, *ip_network));
                    }
                }
            }
        }
        
        // Heuristique basée sur les classes classiques
        let octets = ipv4.octets();
        let prefix = if octets[0] < 128 {
            8
        } else if octets[0] < 192 {
            16
        } else if octets[0] < 224 {
            24
        } else {
            24
        };
        
        let network_octets = match prefix {
            8 => [octets[0], 0, 0, 0],
            16 => [octets[0], octets[1], 0, 0],
            _ => [octets[0], octets[1], octets[2], 0],
        };
        
        // Création d'une adresse IP réseau
        let network_addr = Ipv4Addr::new(
            network_octets[0], network_octets[1], network_octets[2], network_octets[3]
        );
        
        // Création d'un IpNetwork
        match IpNetwork::new(IpAddr::V4(network_addr), prefix) {
            Ok(network) => return Some((*ip, network)),
            Err(_) => return None,
        }
    }
    None
}

/// Obtient le réseau associé à une adresse IP
fn get_network_for_ip(ip: &IpAddr) -> Option<(IpAddr, IpNetwork)> {
    get_network_for_ip_improved(ip)
}

/// Met à jour la table de routage en fonction des informations d'un message LSA
async fn update_routing_from_lsa(
    state: Arc<AppState>,
    lsa: &LSAMessage,
    sender_ip: &str,
    socket: &UdpSocket
) -> Result<()> {
    if lsa.ttl == 0 {
        debug!("Ignoring LSA with expired TTL");
        return Ok(());
    }

    let mut routing_table = state.routing_table.lock().await;
    let next_hop = sender_ip.to_string();

    // Construire une route vers le réseau de l'originator
    if lsa.originator != state.local_ip {
        if let Ok(ip_addr) = lsa.originator.parse::<IpAddr>() {
            if let Some((_, network_cidr)) = get_network_for_ip(&ip_addr) {
                let network_str = network_cidr.to_string();
                let existing_entry = routing_table.get(&network_str);
                // S'il n'y a pas de route ou si la route actuelle est moins intéressante
                let should_update = match existing_entry {
                    None => true,
                    Some((_, RouteState::Unreachable)) => true,
                    Some((_, RouteState::Active(current_metric))) => {
                        // On ne connaît pas la métrique de l'originator ici,
                        // donc on choisit de mettre 1 par défaut ou +1 si on veut
                        // refléter la distance
                        *current_metric > 1
                    }
                };
                if should_update {
                    routing_table.insert(network_str.clone(), (next_hop.clone(), RouteState::Active(1)));
                    if let Err(e) = update_routing_table_safe(&network_str, &next_hop).await {
                        warn!("Could not update system routing table for {}: {}", network_str, e);
                    }
                }
            }
        }
    }

    // Mettre à jour les routes vers tous les voisins de la LSA
    for neighbor in &lsa.neighbors {
        if neighbor.link_up {
            if neighbor.neighbor_ip == state.local_ip {
                continue; // Éviter de s'auto-ajouter
            }
            if let Ok(ip_addr) = neighbor.neighbor_ip.parse::<IpAddr>() {
                if let Some((_, network_cidr)) = get_network_for_ip(&ip_addr) {
                    let network_str = network_cidr.to_string();
                    // Vérifier si on doit mettre à jour la route
                    let existing_entry = routing_table.get(&network_str);
                    let should_update = match existing_entry {
                        None => true,
                        Some((_, RouteState::Unreachable)) => true,
                        Some((_, RouteState::Active(metric))) => *metric > 1,
                    };
                    
                    if should_update {
                        routing_table.insert(network_str.clone(), (next_hop.clone(), RouteState::Active(1)));
                        if let Err(e) = update_routing_table_safe(&network_str, &next_hop).await {
                            warn!("Could not update system routing table for {}: {}", network_str, e);
                        }
                    }
                }
            }
        } else {
            // Gérer les voisins inactifs
            info!("Processing link DOWN for neighbor {} via {}", neighbor.neighbor_ip, sender_ip);
            let mut routes_to_update = Vec::new();
            for (dest, (nh, _)) in routing_table.iter() {
                if nh == sender_ip {
                    routes_to_update.push(dest.clone());
                }
            }
            for dest in routes_to_update {
                info!("Marking route to {} as unreachable (link DOWN)", dest);
                routing_table.insert(dest.clone(), (sender_ip.to_string(), RouteState::Unreachable));
                let handle = match Handle::new() {
                    Ok(h) => h,
                    Err(e) => {
                        warn!("Cannot create routing handle (permissions?): {}", e);
                        continue;
                    }
                };
                let (destination_ip, prefix) = if dest.contains('/') {
                    match dest.parse::<IpNetwork>() {
                        Ok(network) => (network.ip(), network.prefix()),
                        Err(_) => continue,
                    }
                } else {
                    match dest.parse::<IpAddr>() {
                        Ok(ip) => (ip, 32),
                        Err(_) => continue,
                    }
                };
                let route = Route::new(destination_ip, prefix);
                if let Err(e) = handle.delete(&route).await {
                    debug!("Failed to remove route {} from system table: {}", dest, e);
                } else {
                    debug!("Removed route {} from system table", dest);
                }
            }
            if let Err(e) = run_spf_algorithm(&state).await {
                error!("Failed to run SPF algorithm after link DOWN: {}", e);
            }
        }
    }

    // Mettre à jour la table de routage en se basant sur la routing_table de la LSA
    for (dest, route_state) in &lsa.routing_table {
        if dest == &state.local_ip {
            continue;
        }
        match route_state {
            RouteState::Active(metric) => {
                let existing_entry = routing_table.get(dest);
                let new_metric = metric + 1;
                let should_update = match existing_entry {
                    None => true,
                    Some((_, RouteState::Unreachable)) => true,
                    Some((_, RouteState::Active(current_metric))) => new_metric < *current_metric,
                };
                if should_update {
                    routing_table.insert(dest.clone(), (next_hop.clone(), RouteState::Active(new_metric)));
                    if let Err(e) = update_routing_table_safe(dest, &next_hop).await {
                        warn!("Could not update system routing table for {}: {}", dest, e);
                    }
                }
            }
            RouteState::Unreachable => {
                if let Some((current_next_hop, _)) = routing_table.get(dest) {
                    if current_next_hop == sender_ip {
                        routing_table.insert(dest.clone(), (sender_ip.to_string(), RouteState::Unreachable));
                        let handle = match Handle::new() {
                            Ok(h) => h,
                            Err(e) => {
                                warn!("Cannot create routing handle: {}", e);
                                continue;
                            }
                        };
                        let (destination_ip, prefix) = if dest.contains('/') {
                            match dest.parse::<IpNetwork>() {
                                Ok(network) => (network.ip(), network.prefix()),
                                Err(_) => continue,
                            }
                        } else {
                            match dest.parse::<IpAddr>() {
                                Ok(ip) => (ip, 32),
                                Err(_) => continue,
                            }
                        };
                        let route = Route::new(destination_ip, prefix);
                        let _ = handle.delete(&route).await;
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

/// Version sécurisée pour mettre à jour la table de routage système
async fn update_routing_table_safe(destination: &str, gateway: &str) -> Result<()> {
    if gateway == "0.0.0.0" {
        debug!("Skipping connected route: {}", destination);
        return Ok(());
    }

    let (destination_ip, prefix) = if destination.contains('/') {
        match destination.parse::<IpNetwork>() {
            Ok(network) => (network.ip(), network.prefix()),
            Err(_) => return Err(AppError::RouteError("Invalid destination format".to_string())),
        }
    } else {
        match destination.parse::<IpAddr>() {
            Ok(ip) => (ip, 32),
            Err(_) => return Err(AppError::RouteError("Invalid destination IP".to_string())),
        }
    };

    let gateway_ip = match gateway.parse::<IpAddr>() {
        Ok(ip) => ip,
        Err(_) => return Err(AppError::RouteError("Invalid gateway IP".to_string())),
    };

    let handle = match Handle::new() {
        Ok(h) => h,
        Err(e) => {
            warn!("Cannot create routing handle (permissions?): {}", e);
            return Err(AppError::RouteError(format!("Cannot create routing handle: {}", e)));
        }
    };
    
    let route = Route::new(destination_ip, prefix).with_gateway(gateway_ip);

    match handle.add(&route).await {
        Ok(_) => {
            info!("Successfully added route to {}/{} via {}", destination_ip, prefix, gateway_ip);
            Ok(())
        },
        Err(e) => {
            debug!("Route add failed: {}, trying to update", e);
            match handle.delete(&route).await {
                Ok(_) => debug!("Successfully deleted existing route"),
                Err(e) => debug!("No existing route to delete: {}", e),
            }
            match handle.add(&route).await {
                Ok(_) => Ok(()),
                Err(e2) => {
                    Err(AppError::RouteError(format!("Could not add or update route: {}", e2)))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_route_state_serialization() {
        let active = RouteState::Active(10);
        let unreachable = RouteState::Unreachable;
        
        let serialized_active = serde_json::to_string(&active).unwrap();
        let serialized_unreachable = serde_json::to_string(&unreachable).unwrap();
        
        let deserialized_active: RouteState = serde_json::from_str(&serialized_active).unwrap();
        let deserialized_unreachable: RouteState = serde_json::from_str(&serialized_unreachable).unwrap();
        
        assert_eq!(active, deserialized_active);
        assert_eq!(unreachable, deserialized_unreachable);
    }
    
    #[test]
    fn test_calculate_broadcast_for_interface() {
        let ipv4_network = IpNetwork::V4("192.168.1.10/24".parse().unwrap());
        let result = calculate_broadcast_for_interface("192.168.1.10", &ipv4_network, 5000);
        assert!(result.is_ok());
        
        let broadcast_addr = result.unwrap();
        assert_eq!(broadcast_addr.port(), 5000);
        assert_eq!(broadcast_addr.ip(), IpAddr::V4("192.168.1.255".parse().unwrap()));
    }
    
    #[test]
    fn test_app_error_display() {
        let network_err = AppError::NetworkError("Test error".to_string());
        assert_eq!(format!("{}", network_err), "Network error: Test error");
        
        let routing_err = AppError::RoutingError("Routing failed".to_string());
        assert_eq!(format!("{}", routing_err), "Routing error: Routing failed");
    }
}

use std::collections::BinaryHeap;
use std::cmp::Ordering;

/// Structure pour représenter un lien dans le graphe
#[derive(Clone, Debug)]
struct Link {
    to_router: String,
    metric: u32,
}

/// Structure pour les éléments de la file de priorité de Dijkstra
#[derive(Clone, Debug, Eq)]
struct DijkstraNode {
    router_id: String,
    cost: u32,
    next_hop: Option<String>,
}

impl Ord for DijkstraNode {
    fn cmp(&self, other: &Self) -> Ordering {
        other.cost.cmp(&self.cost)
    }
}

impl PartialOrd for DijkstraNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for DijkstraNode {
    fn eq(&self, other: &Self) -> bool {
        self.cost == other.cost
    }
}

/// Fonction pour exécuter l'algorithme Dijkstra et mettre à jour la table de routage
async fn run_spf_algorithm(state: &Arc<AppState>) -> Result<()> {
    info!("Running SPF algorithm to recalculate routes");
    
    let topology_guard = state.topology.lock().await;
    let mut graph = HashMap::new();
    
    // Construire le graphe à partir de la topologie
    for (router_id, router) in topology_guard.iter() {
        let mut links = Vec::new();
        for neighbor in &router.neighbors {
            if neighbor.link_up {
                let metric = if neighbor.capacity >= 100 {
                    1
                } else if neighbor.capacity > 0 {
                    (100 / neighbor.capacity.max(1)).min(15) as u32
                } else {
                    INFINITE_METRIC
                };
                links.push(Link {
                    to_router: neighbor.neighbor_ip.clone(),
                    metric,
                });
            }
        }
        graph.insert(router_id.clone(), links);
    }
    
    // Ajouter le routeur local au graphe
    let mut local_links = Vec::new();
    let neighbors_guard = state.neighbors.lock().await;
    for (neighbor_ip, neighbor) in neighbors_guard.iter() {
        if neighbor.link_up {
            let metric = if neighbor.capacity >= 100 {
                1
            } else if neighbor.capacity > 0 {
                (100 / neighbor.capacity.max(1)).min(15) as u32
            } else {
                INFINITE_METRIC
            };
            local_links.push(Link {
                to_router: neighbor_ip.clone(),
                metric,
            });
        }
    }
    graph.insert(state.router_id.clone(), local_links);
    drop(neighbors_guard);
    drop(topology_guard);
    
    // Exécuter Dijkstra
    let mut distances = HashMap::new();
    let mut next_hops = HashMap::new();
    let mut pq = BinaryHeap::new();
    
    pq.push(DijkstraNode {
        router_id: state.router_id.clone(),
        cost: 0,
        next_hop: None,
    });
    distances.insert(state.router_id.clone(), 0);
    
    while let Some(DijkstraNode { router_id, cost, next_hop }) = pq.pop() {
        if let Some(&dist) = distances.get(&router_id) {
            if cost > dist {
                continue;
            }
        }
        let real_next_hop = next_hop.clone().unwrap_or_else(|| router_id.clone());
        if router_id != state.router_id {
            next_hops.insert(router_id.clone(), real_next_hop.clone());
        }
        if let Some(links) = graph.get(&router_id) {
            for link in links {
                let new_cost = cost + link.metric;
                let is_better_path = match distances.get(&link.to_router) {
                    Some(&old_cost) => new_cost < old_cost,
                    None => true,
                };
                if is_better_path {
                    distances.insert(link.to_router.clone(), new_cost);
                    let nh = if router_id == state.router_id {
                        Some(link.to_router.clone())
                    } else {
                        Some(real_next_hop.clone())
                    };
                    pq.push(DijkstraNode {
                        router_id: link.to_router.clone(),
                        cost: new_cost,
                        next_hop: nh,
                    });
                }
            }
        }
    }
    
    update_routing_table_from_spf(state, &distances, &next_hops).await?;
    Ok(())
}

/// Met à jour la table de routage à partir des résultats de l'algorithme SPF
async fn update_routing_table_from_spf(
    state: &Arc<AppState>,
    distances: &HashMap<String, u32>,
    next_hops: &HashMap<String, String>
) -> Result<()> {
    let mut routing_table = state.routing_table.lock().await;
    
    // Conserver seulement les routes directement connectées
    let connected_routes: Vec<String> = routing_table.iter()
        .filter(|(_, (nh, st))| nh == "0.0.0.0" && matches!(st, RouteState::Active(_)))
        .map(|(dest, _)| dest.clone())
        .collect();
    routing_table.clear();
    
    // Restaurer les routes connectées
    for route in &connected_routes {
        routing_table.insert(route.clone(), ("0.0.0.0".to_string(), RouteState::Active(0)));
    }
    
    // Récupérer tous les réseaux annoncés
    let topology_guard = state.topology.lock().await;
    let mut router_networks = HashMap::new();
    for (router_id, router) in topology_guard.iter() {
        if let Ok(ip) = router_id.parse::<IpAddr>() {
            if let Some((_, network)) = get_network_for_ip_improved(&ip) {
                router_networks.insert(network.to_string(), (router_id.clone(), 0));
            }
        }
        for neighbor in &router.neighbors {
            if neighbor.link_up {
                if let Ok(ip) = neighbor.neighbor_ip.parse::<IpAddr>() {
                    if let Some((_, network)) = get_network_for_ip_improved(&ip) {
                        router_networks.insert(network.to_string(), (router_id.clone(), 0));
                    }
                }
            }
        }
    }
    drop(topology_guard);
    
    // Ajouter les routes calculées
    for (network, (router_id, _)) in router_networks {
        if let Some(cost) = distances.get(&router_id) {
            if let Some(nh_router) = next_hops.get(&router_id) {
                if let Ok(nh_ip) = get_ip_for_router_id(state, nh_router).await {
                    let metric = *cost;
                    routing_table.insert(network.clone(), (nh_ip.clone(), RouteState::Active(metric)));
                    let _ = update_routing_table_safe(&network, &nh_ip).await;
                }
            }
        }
    }
    
    Ok(())
}

/// Obtenir tous les réseaux connus d'un routeur (exemple minimal)
fn get_all_networks_for_router(router: &Router) -> Option<Vec<IpNetwork>> {
    let mut networks = Vec::new();
    if let Ok(ip) = router.router_ip.parse::<IpAddr>() {
        if let Some((_, network)) = get_network_for_ip_improved(&ip) {
            networks.push(network);
        }
    }
    for neighbor in &router.neighbors {
        if neighbor.link_up {
            if let Ok(ip) = neighbor.neighbor_ip.parse::<IpAddr>() {
                if let Some((_, network)) = get_network_for_ip_improved(&ip) {
                    if !networks.contains(&network) {
                        networks.push(network);
                    }
                }
            }
        }
    }
    if networks.is_empty() {
        None
    } else {
        Some(networks)
    }
}

/// Fonction pour obtenir l'IP correspondant à un Router ID
async fn get_ip_for_router_id(state: &Arc<AppState>, router_id: &str) -> Result<String> {
    // Si c'est notre Router ID, trouver une IP locale appropriée
    if router_id == state.router_id {
        return Ok(state.local_ip.clone());
    }
    
    // Chercher parmi les voisins directs
    let neighbors = state.neighbors.lock().await;
    for (ip, neighbor) in neighbors.iter() {
        if neighbor.neighbor_ip == router_id {
            return Ok(ip.clone());
        }
    }
    
    // Si pas trouvé, utiliser le router_id comme IP (ils pourraient être identiques)
    Ok(router_id.to_string())
}
