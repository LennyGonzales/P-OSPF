// Module d'implémentation de l'algorithme de Dijkstra pour OSPF
// Calcul des meilleurs chemins basé sur les coûts, nombre de sauts et capacités

use std::collections::{HashMap, BinaryHeap, HashSet};
use std::cmp::Ordering;
use std::sync::Arc;
use log::{info, debug, warn, error};
use crate::types::{RouteState, Neighbor};
use crate::error::{AppError, Result};
use crate::AppState;
use futures::stream::TryStreamExt;

/// Représente un nœud dans le graphe du réseau
#[derive(Debug, Clone)]
pub struct NetworkNode {
    pub router_id: String,
    pub interfaces: Vec<InterfaceInfo>,
    pub is_reachable: bool,
}

/// Informations sur une interface réseau
#[derive(Debug, Clone)]
pub struct InterfaceInfo {
    pub name: String,
    pub network: String,
    pub capacity_mbps: u32,
    pub is_active: bool,
    pub connected_to: Option<String>, // IP du routeur voisin
}

/// Représente une arête (lien) dans le graphe
#[derive(Debug, Clone)]
pub struct NetworkLink {
    pub from: String,
    pub to: String,
    pub cost: u32,
    pub capacity_mbps: u32,
    pub is_active: bool,
    pub hop_count: u32,
}

/// Nœud utilisé dans l'algorithme de Dijkstra
#[derive(Debug, Clone, Eq, PartialEq)]
struct DijkstraNode {
    router_id: String,
    total_cost: u32,
    hop_count: u32,
    bottleneck_capacity: u32, // Capacité du goulot d'étranglement (lien le plus lent)
    path: Vec<String>,
}

impl Ord for DijkstraNode {
    fn cmp(&self, other: &Self) -> Ordering {
        // Priorité correcte basée sur le coût OSPF, pas le nombre de sauts
        other.total_cost.cmp(&self.total_cost)
            // En cas d'égalité de coût, préférer le chemin avec moins de sauts
            .then_with(|| other.hop_count.cmp(&self.hop_count)) 
            // En cas d'égalité des sauts, préférer le lien avec la meilleure capacité
            .then_with(|| self.bottleneck_capacity.cmp(&other.bottleneck_capacity))
    }
}

impl PartialOrd for DijkstraNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Structure représentant la topologie complète du réseau
#[derive(Debug, Clone)]
pub struct NetworkTopology {
    pub nodes: HashMap<String, NetworkNode>,
    pub links: Vec<NetworkLink>,
}

impl NetworkTopology {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            links: Vec::new(),
        }
    }

    /// Ajoute un routeur à la topologie
    pub fn add_router(&mut self, router_id: String, interfaces: Vec<InterfaceInfo>) {
        let node = NetworkNode {
            router_id: router_id.clone(),
            interfaces,
            is_reachable: true,
        };
        self.nodes.insert(router_id, node);
    }

    /// Ajoute un lien bidirectionnel entre deux routeurs
    pub fn add_link(&mut self, from: String, to: String, capacity_mbps: u32, is_active: bool) {
        let cost = calculate_ospf_cost(capacity_mbps, is_active);
        // Lien direct
        self.links.push(NetworkLink {
            from: from.clone(),
            to: to.clone(),
            cost,
            capacity_mbps,
            is_active,
            hop_count: 1,
        });
        // Lien de retour (bidirectionnel)
        self.links.push(NetworkLink {
            from: to,
            to: from,
            cost,
            capacity_mbps,
            is_active,
            hop_count: 1,
        });
    }

    /// Ajoute un lien bidirectionnel entre deux routeurs
    /// Correction : utilise la capacité minimale entre les deux interfaces pour le coût OSPF
    pub fn add_link_with_min_capacity(&mut self, from: String, to: String, local_capacity: u32, neighbor_capacity: u32, is_active: bool) {
        let min_capacity = local_capacity.min(neighbor_capacity);
        let cost = calculate_ospf_cost(min_capacity, is_active);
        // Lien direct
        self.links.push(NetworkLink {
            from: from.clone(),
            to: to.clone(),
            cost,
            capacity_mbps: min_capacity,
            is_active,
            hop_count: 1,
        });
        // Lien de retour (bidirectionnel)
        self.links.push(NetworkLink {
            from: to,
            to: from,
            cost,
            capacity_mbps: min_capacity,
            is_active,
            hop_count: 1,
        });
    }

    /// Trouve les voisins actifs d'un routeur
    pub fn get_active_neighbors(&self, router_id: &str) -> Vec<&NetworkLink> {
        self.links.iter()
            .filter(|link| link.from == router_id && link.is_active)
            .collect()
    }

    /// Trouve un lien direct entre deux routeurs s'il existe
    pub fn find_link(&self, from: &str, to: &str) -> Option<&NetworkLink> {
        self.links.iter()
            .find(|link| link.from == from && link.to == to)
    }

    /// Calcule les meilleurs chemins depuis un routeur source
    /// Basé sur : 1) Plus court chemin (nombre de sauts), 2) Capacité goulot, 3) État des liens
    pub fn calculate_shortest_paths(&self, source: &str) -> HashMap<String, RouteInfo> {
        let mut costs: HashMap<String, u32> = HashMap::new();
        let mut hop_counts: HashMap<String, u32> = HashMap::new();
        let mut bottleneck_capacities: HashMap<String, u32> = HashMap::new();
        let mut paths: HashMap<String, Vec<String>> = HashMap::new();
        let mut visited = HashSet::new();
        let mut heap = BinaryHeap::new();

        // Initialisation avec des valeurs infinies
        for node_id in self.nodes.keys() {
            costs.insert(node_id.clone(), u32::MAX);
            hop_counts.insert(node_id.clone(), u32::MAX);
            bottleneck_capacities.insert(node_id.clone(), 0);
            paths.insert(node_id.clone(), Vec::new());
        }

        // Nœud source
        costs.insert(source.to_string(), 0);
        hop_counts.insert(source.to_string(), 0);
        bottleneck_capacities.insert(source.to_string(), u32::MAX);
        paths.insert(source.to_string(), vec![source.to_string()]);

        heap.push(DijkstraNode {
            router_id: source.to_string(),
            total_cost: 0,  // Coût total commence à 0
            hop_count: 0,
            bottleneck_capacity: u32::MAX,
            path: vec![source.to_string()],
        });

        // Algorithme de Dijkstra
        while let Some(current) = heap.pop() {
            if visited.contains(&current.router_id) {
                continue;
            }
            visited.insert(current.router_id.clone());

            // Explorer les voisins actifs uniquement
            for link in self.get_active_neighbors(&current.router_id) {
                if visited.contains(&link.to) {
                    continue;
                }

                // Calculer le nouveau coût en additionnant le coût du lien
                let new_cost = match current.total_cost.checked_add(link.cost) {
                    Some(cost) => cost,
                    None => continue, // Éviter les débordements pour les chemins infinis
                };
                
                let new_hop_count = current.hop_count + 1;
                let new_bottleneck_capacity = current.bottleneck_capacity.min(link.capacity_mbps);
                
                let current_best_cost = *costs.get(&link.to).unwrap_or(&u32::MAX);

                // Mettre à jour si on a trouvé un chemin avec un meilleur coût OSPF
                if new_cost < current_best_cost {
                    costs.insert(link.to.clone(), new_cost);
                    hop_counts.insert(link.to.clone(), new_hop_count);
                    bottleneck_capacities.insert(link.to.clone(), new_bottleneck_capacity);
                    
                    let mut new_path = current.path.clone();
                    new_path.push(link.to.clone());
                    paths.insert(link.to.clone(), new_path.clone());

                    heap.push(DijkstraNode {
                        router_id: link.to.clone(),
                        total_cost: new_cost,  // Coût total mis à jour
                        hop_count: new_hop_count,
                        bottleneck_capacity: new_bottleneck_capacity,
                        path: new_path,
                    });
                }
            }
        }

        // Construire les résultats
        let mut routes = HashMap::new();
        for (dest, cost) in costs {
            if dest != source && cost != u32::MAX {
                let path = paths.get(&dest).unwrap_or(&Vec::new()).clone();
                let next_hop = if path.len() > 1 { path[1].clone() } else { dest.clone() };
                
                routes.insert(dest.clone(), RouteInfo {
                    destination: dest.clone(),
                    next_hop,
                    total_cost: cost,  // Utiliser le coût OSPF calculé
                    hop_count: *hop_counts.get(&dest).unwrap_or(&0),
                    bottleneck_capacity: *bottleneck_capacities.get(&dest).unwrap_or(&0),
                    path,
                    is_reachable: true,
                });
            }
        }

        routes
    }
}

/// Informations sur une route calculée
#[derive(Debug, Clone)]
pub struct RouteInfo {
    pub destination: String,
    pub next_hop: String,
    pub total_cost: u32,
    pub hop_count: u32,
    pub bottleneck_capacity: u32,
    pub path: Vec<String>,
    pub is_reachable: bool,
}

/// Calcule le coût OSPF basé sur la capacité et l'état
pub fn calculate_ospf_cost(capacity_mbps: u32, is_active: bool) -> u32 {
    // Coût infini pour les liens inactifs
    if !is_active {
        return u32::MAX;
    }
    
    // Éviter la division par zéro
    if capacity_mbps == 0 {
        return u32::MAX;
    }
    
    // Formule OSPF standard : référence de 100 Mbps (100 000 000 bps)
    // Le coût OSPF est l'inverse de la bande passante
    let reference_bandwidth = 100_000_000u64; // 100 Mbps en bps
    let bandwidth_bps = capacity_mbps as u64 * 1_000_000;
    
    // Vérification supplémentaire contre la division par zéro
    if bandwidth_bps == 0 {
        return u32::MAX;
    }
    
    // Utiliser u64 pour éviter les problèmes de dépassement
    let cost = (reference_bandwidth / bandwidth_bps) as u32;
    
    // Coût minimum de 1 (même pour les liens très rapides)
    cost.max(1)
}

/// Construit la topologie réseau à partir de l'état OSPF
pub async fn build_network_topology(state: Arc<AppState>) -> NetworkTopology {
    let mut topology = NetworkTopology::new();
    
    // Ajouter le routeur local
    let local_interfaces = state.config.interfaces.iter().map(|iface| {
        InterfaceInfo {
            name: iface.name.clone(),
            network: format!("network_{}", iface.name), // Simplification
            capacity_mbps: iface.capacity_mbps,
            is_active: iface.link_active,
            connected_to: None,
        }
    }).collect();
    
    topology.add_router(state.local_ip.clone(), local_interfaces);
    
    // Ajouter les voisins et leurs liens
    let neighbors = state.neighbors.lock().await;
    for (neighbor_ip, neighbor) in neighbors.iter() {
        // Ajouter le voisin s'il n'existe pas
        if !topology.nodes.contains_key(neighbor_ip) {
            topology.add_router(neighbor_ip.clone(), Vec::new());
        }
        
        // Ajouter le lien si le voisin est actif
        if neighbor.link_up {
            topology.add_link_with_min_capacity(
                state.local_ip.clone(),
                neighbor_ip.clone(),
                neighbor.capacity,
                neighbor.capacity, // Utiliser la capacité voisine pour le coût
                true,
            );
        }
    }
    drop(neighbors);
    
    topology
}

/// Calcule et met à jour les routes optimales
pub async fn calculate_and_update_optimal_routes(state: Arc<AppState>) -> Result<()> {
    debug!("Calcul des routes optimales en cours...");
    
    // Construire la topologie
    let topology = build_network_topology(Arc::clone(&state)).await;
    
    // Calculer les meilleurs chemins depuis le routeur local
    let shortest_paths = topology.calculate_shortest_paths(&state.local_ip);
    
    if shortest_paths.is_empty() {
        warn!("Aucune route calculée - routeur probablement isolé");
        return Ok(());
    }
    
    let mut new_routing_table = HashMap::new();
    let mut routes_updated = 0; // Compteur des routes mises à jour
    let lsdb = state.topology.lock().await;

    // Parcourir la LSDB pour trouver les réseaux annoncés
    for (originator, router_state) in lsdb.iter() {
        if let Some(lsa) = &router_state.last_lsa {
            // Trouver le chemin vers cet originator
            if let Some(route_info) = shortest_paths.get(originator) {
                if route_info.is_reachable && route_info.total_cost < u32::MAX {
                    // Parcourir les routes annoncées par cet originator
                    for (network_prefix, route_state) in &lsa.routing_table {
                        if let RouteState::Active(metric) = route_state {
                            // Calculer le coût total (coût local + métrique distante)
                            let total_metric = if *metric == u32::MAX || route_info.total_cost == u32::MAX {
                                u32::MAX
                            } else {
                                route_info.total_cost.saturating_add(*metric)
                            };
                            
                            // Vérifier si cette route est meilleure que celles déjà connues
                            let should_update = match new_routing_table.get(network_prefix) {
                                Some((_, RouteState::Active(current_metric))) => total_metric < *current_metric,
                                Some((_, RouteState::Unreachable)) => true,
                                None => true,
                            };
                            
                            if should_update {
                                routes_updated += 1; // Correction : incrémenter le compteur
                                // Ajouter la route à notre table de routage interne
                                new_routing_table.insert(
                                    network_prefix.clone(),
                                    (route_info.next_hop.clone(), RouteState::Active(total_metric)),
                                );
                                
                                // Ne mettre à jour la table système que si le préfixe est valide
                                if network_prefix.contains('/') {
                                    if let Err(e) = crate::lsa::update_routing_table_safe(network_prefix, &route_info.next_hop).await {
                                        warn!("Échec de la mise à jour de la route système vers {} via {}: {}", 
                                              network_prefix, &route_info.next_hop, e);
                                    } else {
                                        info!("Route mise à jour: {} via {} (coût: {})", 
                                              network_prefix, &route_info.next_hop, total_metric);
                                    }
                                } else {
                                    debug!("Préfixe invalide ignoré: {}", network_prefix);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Mise à jour complète de la table de routage
    let mut routing_table = state.routing_table.lock().await;
    *routing_table = new_routing_table;
    
    info!("Calcul des routes terminé. {} routes dans la table de routage ({} mises à jour).", 
          routing_table.len(), routes_updated);
    Ok(())
}

/// Met à jour une route dans la table de routage système
async fn update_system_route(destination: &str, gateway: &str) -> Result<()> {
    use rtnetlink::{new_connection, IpVersion};
    use std::net::Ipv4Addr;
    use tokio::time::{timeout, Duration};
    use pnet::ipnetwork::IpNetwork;

    // Vérifier que la destination est un réseau avec préfixe (CIDR)
    if !destination.contains('/') {
        return Err(AppError::RouteError(format!("Format de destination invalide (CIDR attendu): {}", destination)));
    }

    // Analyser le réseau de destination
    let network: IpNetwork = destination.parse()
        .map_err(|e| AppError::RouteError(format!("Analyse du réseau destination échouée {}: {}", destination, e)))?;

    // Extraire l'adresse et le préfixe
    let (dest_ip, prefix_len) = match network {
        IpNetwork::V4(ipv4) => (ipv4.network(), ipv4.prefix()),
        IpNetwork::V6(_) => return Err(AppError::RouteError("IPv6 non supporté".to_string())),
    };

    // Analyser la passerelle
    let gw_ip: Ipv4Addr = gateway.parse()
        .map_err(|e| AppError::RouteError(format!("Passerelle IPv4 invalide {}: {}", gateway, e)))?;

    // Vérifier la validité de la passerelle
    if gw_ip.is_unspecified() || gw_ip.is_broadcast() || gw_ip.is_loopback() {
        return Err(AppError::RouteError(format!("Adresse de passerelle invalide: {}", gw_ip)));
    }

    // Créer une connexion netlink
    let (connection, handle, _) = match new_connection() {
        Ok(conn) => conn,
        Err(e) => return Err(AppError::RouteError(format!("Échec de connexion netlink: {}", e))),
    };
    tokio::spawn(connection);

    // Essayer d'abord de supprimer la route existante
    let mut routes = handle.route().get(IpVersion::V4).execute();
    let mut route_existed = false;
    
    while let Ok(Ok(Some(route))) = timeout(Duration::from_secs(1), routes.try_next()).await {
        if route.destination_prefix() == Some((std::net::IpAddr::V4(dest_ip), prefix_len as u8)) {
            route_existed = true;
            match handle.route().del(route).execute().await {
                Ok(_) => debug!("Route existante supprimée: {} via {}", destination, gateway),
                Err(e) => debug!("Erreur lors de la suppression de la route existante: {}", e),
            }
        }
    }

    // Ajouter la nouvelle route avec un message approprié
    let add_route = handle.route().add()
        .v4()
        .destination_prefix(dest_ip, prefix_len as u8)
        .gateway(gw_ip)
        .execute();

    match timeout(Duration::from_secs(2), add_route).await {
        Ok(Ok(_)) => {
            let action = if route_existed { "mise à jour" } else { "ajoutée" };
            info!("Route système {}: {} via {}", action, destination, gateway);
            Ok(())
        }
        Ok(Err(e)) => {
            error!("Erreur netlink lors de l'ajout de la route: {}", e);
            Err(AppError::RouteError(format!("Erreur netlink: {}", e)))
        }
        Err(_) => {
            error!("Timeout netlink lors de l'ajout de la route");
            Err(AppError::RouteError("Timeout netlink".into()))
        }
    }
}