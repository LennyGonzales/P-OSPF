// Module d'implémentation de l'algorithme de Dijkstra pour OSPF
// Nouvelle implémentation complète pour calculer les chemins optimaux

use std::collections::{HashMap, BinaryHeap, HashSet};
use std::cmp::Ordering;
use std::sync::Arc;
use log::{info, debug, warn, error};
use crate::types::{RouteState, Neighbor};
use crate::error::{AppError, Result};
use crate::AppState;

/// Représente un routeur dans la topologie réseau
#[derive(Debug, Clone)]
pub struct RouterNode {
    pub router_id: String,
    pub interfaces: Vec<InterfaceInfo>,
    pub is_active: bool,
    pub area_id: String,
}

/// Informations détaillées sur une interface réseau
#[derive(Debug, Clone)]
pub struct InterfaceInfo {
    pub interface_name: String,
    pub ip_address: String,
    pub network_prefix: String,
    pub subnet_mask: String,
    pub capacity_mbps: u32,
    pub is_up: bool,
    pub connected_router: Option<String>,
    pub metric: u32,
}

/// Lien bidirectionnel entre deux routeurs
#[derive(Debug, Clone)]
pub struct NetworkLink {
    pub router_a: String,
    pub router_b: String,
    pub cost: u32,
    pub bandwidth_mbps: u32,
    pub is_operational: bool,
    pub delay_ms: u32,
    pub interface_a: String,
    pub interface_b: String,
}

/// Nœud dans l'algorithme de Dijkstra avec critères multiples
#[derive(Debug, Clone, Eq, PartialEq)]
struct PathNode {
    destination: String,
    accumulated_cost: u32,
    hop_count: u32,
    min_bandwidth: u32,
    max_delay: u32,
    path_routers: Vec<String>,
    next_hop: String,
}

impl Ord for PathNode {
    fn cmp(&self, other: &Self) -> Ordering {
        // Critères de comparaison OSPF standards
        // 1. Coût accumulé (priorité principale)
        // 2. Nombre de sauts (tie-breaker)
        // 3. Bande passante minimale (qualité du chemin)
        other.accumulated_cost.cmp(&self.accumulated_cost)
            .then_with(|| other.hop_count.cmp(&self.hop_count))
            .then_with(|| self.min_bandwidth.cmp(&other.min_bandwidth))
    }
}

impl PartialOrd for PathNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Topologie complète du réseau avec gestion avancée
#[derive(Debug, Clone)]
pub struct NetworkTopology {
    pub routers: HashMap<String, RouterNode>,
    pub links: HashMap<String, NetworkLink>, // Clé: "routerA-routerB"
    pub networks: HashMap<String, NetworkInfo>,
    pub area_id: String,
    pub last_update: u64,
}

/// Informations sur un réseau/subnet
#[derive(Debug, Clone)]
pub struct NetworkInfo {
    pub network_id: String,
    pub prefix: String,
    pub connected_routers: Vec<String>,
    pub network_type: NetworkType,
}

#[derive(Debug, Clone)]
pub enum NetworkType {
    PointToPoint,
    Broadcast,
    NBMA,
    PointToMultipoint,
}

impl NetworkTopology {
    /// Crée une nouvelle topologie vide
    pub fn new(area_id: String) -> Self {
        Self {
            routers: HashMap::new(),
            links: HashMap::new(),
            networks: HashMap::new(),
            area_id,
            last_update: current_timestamp(),
        }
    }

    /// Ajoute un routeur à la topologie
    pub fn add_router(&mut self, router: RouterNode) {
        info!("Ajout du routeur {} avec {} interfaces", router.router_id, router.interfaces.len());
        self.routers.insert(router.router_id.clone(), router);
        self.last_update = current_timestamp();
    }

    /// Ajoute ou met à jour un lien entre deux routeurs
    pub fn add_or_update_link(&mut self, router_a: String, router_b: String, 
                             bandwidth_mbps: u32, is_operational: bool,
                             interface_a: String, interface_b: String) {
        let link_key = create_link_key(&router_a, &router_b);
        let cost = calculate_link_cost(bandwidth_mbps, is_operational);
        
        let link = NetworkLink {
            router_a: router_a.clone(),
            router_b: router_b.clone(),
            cost,
            bandwidth_mbps,
            is_operational,
            delay_ms: calculate_propagation_delay(bandwidth_mbps),
            interface_a,
            interface_b,
        };

        debug!("Lien {} : coût={}, bande passante={}Mbps, état={}", 
               link_key, cost, bandwidth_mbps, is_operational);
        
        self.links.insert(link_key, link);
        self.last_update = current_timestamp();
    }

    /// Obtient tous les voisins actifs d'un routeur
    pub fn get_active_neighbors(&self, router_id: &str) -> Vec<(String, &NetworkLink)> {
        let mut neighbors = Vec::new();
        
        for (_, link) in &self.links {
            if !link.is_operational {
                continue;
            }
            
            if link.router_a == router_id {
                neighbors.push((link.router_b.clone(), link));
            } else if link.router_b == router_id {
                neighbors.push((link.router_a.clone(), link));
            }
        }
        
        neighbors
    }

    /// Implémentation complète de l'algorithme de Dijkstra
    pub fn compute_shortest_paths(&self, source_router: &str) -> HashMap<String, OptimalRoute> {
        debug!("Calcul des chemins optimaux depuis {}", source_router);
        
        if !self.routers.contains_key(source_router) {
            error!("Routeur source {} introuvable dans la topologie", source_router);
            return HashMap::new();
        }

        let mut distances: HashMap<String, u32> = HashMap::new();
        let mut previous: HashMap<String, String> = HashMap::new();
        let mut visited: HashSet<String> = HashSet::new();
        let mut priority_queue = BinaryHeap::new();

        // Initialisation des distances
        for router_id in self.routers.keys() {
            distances.insert(router_id.clone(), u32::MAX);
        }
        distances.insert(source_router.to_string(), 0);

        // Ajouter le nœud source à la file de priorité
        priority_queue.push(PathNode {
            destination: source_router.to_string(),
            accumulated_cost: 0,
            hop_count: 0,
            min_bandwidth: u32::MAX,
            max_delay: 0,
            path_routers: vec![source_router.to_string()],
            next_hop: source_router.to_string(),
        });

        // Algorithme de Dijkstra principal
        while let Some(current_node) = priority_queue.pop() {
            if visited.contains(&current_node.destination) {
                continue;
            }
            
            visited.insert(current_node.destination.clone());
            
            // Explorer tous les voisins du nœud actuel
            for (neighbor_id, link) in self.get_active_neighbors(&current_node.destination) {
                if visited.contains(&neighbor_id) {
                    continue;
                }

                let new_cost = current_node.accumulated_cost + link.cost;
                let current_distance = *distances.get(&neighbor_id).unwrap_or(&u32::MAX);

                if new_cost < current_distance {
                    distances.insert(neighbor_id.clone(), new_cost);
                    previous.insert(neighbor_id.clone(), current_node.destination.clone());

                    let mut new_path = current_node.path_routers.clone();
                    new_path.push(neighbor_id.clone());

                    let next_hop = if current_node.destination == source_router {
                        neighbor_id.clone()
                    } else {
                        current_node.next_hop.clone()
                    };

                    priority_queue.push(PathNode {
                        destination: neighbor_id.clone(),
                        accumulated_cost: new_cost,
                        hop_count: current_node.hop_count + 1,
                        min_bandwidth: current_node.min_bandwidth.min(link.bandwidth_mbps),
                        max_delay: current_node.max_delay.max(link.delay_ms),
                        path_routers: new_path,
                        next_hop,
                    });
                }
            }
        }

        // Construire les routes optimales
        self.build_optimal_routes(source_router, &distances, &previous)
    }

    /// Construit les routes optimales à partir des résultats de Dijkstra
    fn build_optimal_routes(&self, source: &str, distances: &HashMap<String, u32>, 
                           previous: &HashMap<String, String>) -> HashMap<String, OptimalRoute> {
        let mut routes = HashMap::new();

        for (destination, &cost) in distances {
            if destination == source || cost == u32::MAX {
                continue;
            }

            // Reconstruire le chemin complet
            let path = self.reconstruct_path(source, destination, previous);
            let next_hop = if path.len() > 1 { path[1].clone() } else { destination.clone() };

            // Calculer les métriques du chemin
            let (min_bandwidth, total_delay) = self.calculate_path_metrics(&path);

            let route = OptimalRoute {
                destination: destination.clone(),
                next_hop,
                total_cost: cost,
                hop_count: path.len().saturating_sub(1) as u32,
                path,
                min_bandwidth,
                total_delay,
                is_valid: true,
                last_updated: current_timestamp(),
            };

            routes.insert(destination.clone(), route);
        }

        info!("Calculé {} routes optimales depuis {}", routes.len(), source);
        routes
    }

    /// Reconstruit le chemin complet vers une destination
    fn reconstruct_path(&self, source: &str, destination: &str, 
                       previous: &HashMap<String, String>) -> Vec<String> {
        let mut path = Vec::new();
        let mut current = destination;

        while current != source {
            path.push(current.to_string());
            if let Some(prev) = previous.get(current) {
                current = prev;
            } else {
                break;
            }
        }
        path.push(source.to_string());
        path.reverse();
        path
    }

    /// Calcule les métriques qualité d'un chemin
    fn calculate_path_metrics(&self, path: &[String]) -> (u32, u32) {
        let mut min_bandwidth = u32::MAX;
        let mut total_delay = 0;

        for i in 0..path.len().saturating_sub(1) {
            let link_key = create_link_key(&path[i], &path[i + 1]);
            if let Some(link) = self.links.get(&link_key) {
                min_bandwidth = min_bandwidth.min(link.bandwidth_mbps);
                total_delay += link.delay_ms;
            }
        }

        (min_bandwidth, total_delay)
    }

    /// Affiche un résumé complet de la topologie
    pub fn display_topology_summary(&self) {
        info!("\n=== RÉSUMÉ DE LA TOPOLOGIE RÉSEAU ===");
        info!("Zone: {} | Routeurs: {} | Liens: {}", 
              self.area_id, self.routers.len(), self.links.len());
        
        info!("\nROUTEURS:");
        for (id, router) in &self.routers {
            let status = if router.is_active { "ACTIF" } else { "INACTIF" };
            info!("  • {} [{}] - {} interfaces", id, status, router.interfaces.len());
            
            for interface in &router.interfaces {
                let if_status = if interface.is_up { "UP" } else { "DOWN" };
                info!("    - {} ({}) {}Mbps [{}]", 
                     interface.interface_name, interface.ip_address, 
                     interface.capacity_mbps, if_status);
            }
        }
        
        info!("\nLIENS:");
        for (key, link) in &self.links {
            let status = if link.is_operational { "OPÉRATIONNEL" } else { "HORS SERVICE" };
            info!("  • {} : {} <-> {} | Coût: {} | {}Mbps [{}]",
                 key, link.router_a, link.router_b, link.cost, 
                 link.bandwidth_mbps, status);
        }
        info!("=====================================\n");
    }
}

/// Route optimale calculée par l'algorithme de Dijkstra
#[derive(Debug, Clone)]
pub struct OptimalRoute {
    pub destination: String,
    pub next_hop: String,
    pub total_cost: u32,
    pub hop_count: u32,
    pub path: Vec<String>,
    pub min_bandwidth: u32,
    pub total_delay: u32,
    pub is_valid: bool,
    pub last_updated: u64,
}

/// Calcule le coût OSPF standardisé d'un lien
pub fn calculate_link_cost(bandwidth_mbps: u32, is_operational: bool) -> u32 {
    if !is_operational || bandwidth_mbps == 0 {
        return u32::MAX; // Lien inutilisable
    }
    
    // Formule OSPF RFC 2328: coût = référence / bande_passante
    const REFERENCE_BANDWIDTH: u32 = 100_000; // 100 Mbps en kbps
    let bandwidth_kbps = bandwidth_mbps * 1000;
    
    let cost = REFERENCE_BANDWIDTH / bandwidth_kbps;
    cost.max(1) // Coût minimum de 1
}

/// Calcule le délai de propagation estimé
fn calculate_propagation_delay(bandwidth_mbps: u32) -> u32 {
    // Délai basé sur la bande passante (approximation)
    match bandwidth_mbps {
        0..=1 => 100,      // Très lent
        2..=10 => 50,      // Lent
        11..=100 => 10,    // Moyen
        101..=1000 => 5,   // Rapide
        _ => 1,            // Très rapide
    }
}

/// Crée une clé unique pour un lien bidirectionnel
fn create_link_key(router_a: &str, router_b: &str) -> String {
    if router_a < router_b {
        format!("{}-{}", router_a, router_b)
    } else {
        format!("{}-{}", router_b, router_a)
    }
}

/// Obtient le timestamp actuel
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_else(|_| std::time::Duration::from_secs(0))
        .as_secs()
}

/// Construit la topologie réseau à partir des LSA reçues
pub async fn build_network_topology_from_lsa(state: Arc<AppState>) -> Result<NetworkTopology> {
    debug!("Construction de la topologie réseau à partir des LSA...");
    
    let mut topology = NetworkTopology::new("0.0.0.0".to_string()); // Zone par défaut
    let lsa_db = state.lsa_db.lock().await;
    
    if lsa_db.is_empty() {
        warn!("Base de données LSA vide - topologie locale uniquement");
        drop(lsa_db);
        return Ok(topology);
    }

    // Première passe: créer tous les routeurs
    for (originator_id, lsa) in lsa_db.iter() {
        let router_id = clean_router_id(originator_id);
        
        // Créer les interfaces du routeur
        let interfaces = create_interfaces_from_neighbors(&lsa.neighbors);
        
        let router_node = RouterNode {
            router_id: router_id.clone(),
            interfaces,
            is_active: true,
            area_id: "0.0.0.0".to_string(),
        };
        
        topology.add_router(router_node);
    }

    // Deuxième passe: créer tous les liens
    for (originator_id, lsa) in lsa_db.iter() {
        let router_id = clean_router_id(originator_id);
        
        for neighbor in &lsa.neighbors {
            let neighbor_id = clean_router_id(&neighbor.neighbor_ip);
            
            // Éviter les doublons de liens
            if router_id < neighbor_id {
                topology.add_or_update_link(
                    router_id.clone(),
                    neighbor_id.clone(),
                    neighbor.capacity,
                    neighbor.link_up,
                    format!("if_{}", neighbor_id),
                    format!("if_{}", router_id),
                );
            }
        }
    }
    
    drop(lsa_db);
    
    // Ajouter le routeur local s'il n'existe pas
    let local_router_id = clean_router_id(&state.local_ip);
    if !topology.routers.contains_key(&local_router_id) {
        let local_interfaces = create_local_interfaces(&state.config).await;
        let local_router = RouterNode {
            router_id: local_router_id,
            interfaces: local_interfaces,
            is_active: true,
            area_id: "0.0.0.0".to_string(),
        };
        topology.add_router(local_router);
    }

    info!("Topologie construite avec {} routeurs et {} liens", 
          topology.routers.len(), topology.links.len());
    
    Ok(topology)
}

/// Nettoie l'ID du routeur (supprime le masque réseau)
fn clean_router_id(router_ip: &str) -> String {
    router_ip.split('/').next().unwrap_or(router_ip).to_string()
}

/// Crée les interfaces à partir des voisins dans une LSA
fn create_interfaces_from_neighbors(neighbors: &[Neighbor]) -> Vec<InterfaceInfo> {
    neighbors.iter().enumerate().map(|(index, neighbor)| {
        InterfaceInfo {
            interface_name: format!("eth{}", index),
            ip_address: format!("{}/24", neighbor.neighbor_ip),
            network_prefix: extract_network_prefix(&neighbor.neighbor_ip),
            subnet_mask: "255.255.255.0".to_string(),
            capacity_mbps: neighbor.capacity,
            is_up: neighbor.link_up,
            connected_router: Some(clean_router_id(&neighbor.neighbor_ip)),
            metric: calculate_link_cost(neighbor.capacity, neighbor.link_up),
        }
    }).collect()
}

/// Crée les interfaces locales à partir de la configuration
async fn create_local_interfaces(config: &crate::read_config::RouterConfig) -> Vec<InterfaceInfo> {
    config.interfaces.iter().enumerate().map(|(index, interface)| {
        InterfaceInfo {
            interface_name: interface.name.clone(),
            ip_address: if !interface.ip.is_empty() {
                format!("{}/{}", interface.ip, interface.mask)
            } else {
                format!("192.168.{}.1/{}", index + 1, interface.mask)
            },
            network_prefix: if !interface.network.is_empty() {
                format!("{}/{}", interface.network, interface.mask)
            } else {
                format!("192.168.{}.0/{}", index + 1, interface.mask)
            },
            subnet_mask: mask_to_dotted_decimal(interface.mask),
            capacity_mbps: interface.capacity,
            is_up: interface.link_active,
            connected_router: None,
            metric: calculate_link_cost(interface.capacity, interface.link_active),
        }
    }).collect()
}

/// Extrait le préfixe réseau d'une adresse IP
fn extract_network_prefix(ip_address: &str) -> String {
    let clean_ip = ip_address.split('/').next().unwrap_or(ip_address);
    let parts: Vec<&str> = clean_ip.split('.').collect();
    if parts.len() >= 3 {
        format!("{}.{}.{}.0/24", parts[0], parts[1], parts[2])
    } else {
        format!("{}/24", clean_ip)
    }
}

/// Convertit un masque CIDR en notation décimale pointée
fn mask_to_dotted_decimal(cidr: u8) -> String {
    let mask = (!0u32) << (32 - cidr);
    format!("{}.{}.{}.{}", 
            (mask >> 24) & 0xFF,
            (mask >> 16) & 0xFF,
            (mask >> 8) & 0xFF,
            mask & 0xFF)
}

/// Fonction principale pour calculer et mettre à jour les routes
pub async fn calculate_and_update_optimal_routes(state: Arc<AppState>) -> Result<()> {
    info!("Début du calcul des routes optimales...");
    
    // Construire la topologie actuelle
    let topology = build_network_topology_from_lsa(Arc::clone(&state)).await?;
    
    // Afficher un résumé de la topologie
    topology.display_topology_summary();
    
    // Calculer les routes optimales
    let local_router_id = clean_router_id(&state.local_ip);
    let optimal_routes = topology.compute_shortest_paths(&local_router_id);
    
    if optimal_routes.is_empty() {
        warn!("Aucune route calculée - routeur isolé ou topologie vide");
        return Ok(());
    }

    // Mettre à jour la table de routage locale
    {
        let mut routing_table = state.routing_table.lock().await;
        routing_table.clear();
        
        for (destination, route) in &optimal_routes {
            routing_table.insert(
                destination.clone(),
                (route.next_hop.clone(), RouteState::Active(route.total_cost)),
            );
        }
        
        info!("Table de routage mise à jour avec {} routes", routing_table.len());
    }

    // Afficher les routes calculées
    display_routing_results(&optimal_routes);
    
    // Optionnel: mettre à jour les routes système
    for (destination, route) in &optimal_routes {
        if let Err(e) = update_system_routing_table(destination, &route.next_hop).await {
            warn!("Échec de la mise à jour système pour {} via {}: {}", 
                  destination, route.next_hop, e);
        }
    }
    
    info!("Calcul des routes optimales terminé avec succès");
    Ok(())
}

/// Affiche les résultats du calcul de routage
fn display_routing_results(routes: &HashMap<String, OptimalRoute>) {
    info!("\n=== ROUTES OPTIMALES CALCULÉES ===");
    
    let mut sorted_routes: Vec<_> = routes.iter().collect();
    sorted_routes.sort_by(|a, b| a.0.cmp(b.0));
    
    for (destination, route) in sorted_routes {
        info!("  {} via {} | Coût: {} | Sauts: {} | Bande passante min: {}Mbps | Délai: {}ms",
              destination, route.next_hop, route.total_cost, route.hop_count,
              route.min_bandwidth, route.total_delay);
        
        if route.path.len() > 2 {
            let path_str = route.path.join(" -> ");
            debug!("    Chemin complet: {}", path_str);
        }
    }
    
    info!("=================================\n");
}

/// Met à jour la table de routage système (placeholder)
async fn update_system_routing_table(destination: &str, next_hop: &str) -> Result<()> {
    // Dans un système réel, ceci interfacerait avec netlink ou ip route
    debug!("Route système: {} via {} [SIMULÉ]", destination, next_hop);
    Ok(())
}