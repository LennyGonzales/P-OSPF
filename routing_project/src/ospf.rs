use crate::types::*;
use std::collections::HashMap;
use std::net::{Ipv4Addr, IpAddr};
use petgraph::algo::dijkstra;
use petgraph::graph::{UnGraph};

pub struct OSPFProtocol {
    config: RouterConfig,
    neighbors: HashMap<Ipv4Addr, Neighbor>,
    links: HashMap<Ipv4Addr, Link>,
    lsa_database: HashMap<Ipv4Addr, RouterLSA>,
    routing_table: Vec<RoutingEntry>,
    sequence_number: u32,
}

impl OSPFProtocol {
    pub fn new(config: RouterConfig) -> Self {
        Self {
            config,
            neighbors: HashMap::new(),
            links: HashMap::new(),
            lsa_database: HashMap::new(),
            routing_table: Vec::new(),
            sequence_number: 1,
        }
    }
    
    pub async fn process_hello(&mut self, hello: HelloPacket, sender_ip: IpAddr) {
        log::info!("Réception Hello de {}", hello.router_id);
        
        // Mettre à jour ou créer le voisin
        let neighbor = self.neighbors.entry(hello.router_id).or_insert(Neighbor {
            router_id: hello.router_id,
            state: NeighborState::Init,
            last_seen: current_timestamp(),
            interface_addr: match sender_ip {
                IpAddr::V4(addr) => addr,
                _ => return,
            },
        });
        
        neighbor.last_seen = current_timestamp();
        
        // Vérifier si notre ID est dans la liste des voisins
        if hello.neighbors.contains(&self.config.router_id) {
            neighbor.state = NeighborState::TwoWay;
            log::info!("Voisin {} est maintenant en état TwoWay", hello.router_id);
        }
        
        // Mettre à jour le lien correspondant si il existe
        if let Some(link) = self.links.get_mut(&hello.router_id) {
            link.last_hello = current_timestamp();
            link.state = LinkState::Up;
        }
    }
    
    pub async fn process_lsa(&mut self, lsa: RouterLSA) {
        log::info!("Réception LSA de {}", lsa.header.advertising_router);
        
        // Vérifier si c'est une nouvelle LSA ou une mise à jour
        let should_update = match self.lsa_database.get(&lsa.header.advertising_router) {
            Some(existing) => lsa.header.sequence_number > existing.header.sequence_number,
            None => true,
        };
        
        if should_update {
            self.lsa_database.insert(lsa.header.advertising_router, lsa);
            log::info!("LSA mise à jour dans la base de données");
            
            // Recalculer la table de routage
            self.calculate_routing_table().await;
            
            // TODO: Propager la LSA aux autres voisins (flooding)
        }
    }
    
    pub async fn process_lsa_ack(&self, _headers: Vec<LSAHeader>) {
        // Traitement des accusés de réception LSA
        println!("Réception LSA ACK");
    }
    
    pub async fn send_hello_packets(&self) -> Result<(), Box<dyn std::error::Error>> {
        let hello = HelloPacket {
            router_id: self.config.router_id,
            area_id: 0, // Area backbone
            hello_interval: self.config.hello_interval,
            dead_interval: self.config.dead_interval,
            neighbors: self.neighbors.keys().cloned().collect(),
            timestamp: current_timestamp(),
        };
        
        let message = OSPFMessage::Hello(hello);
        let serialized = serde_json::to_string(&message)?;
        
        println!("Envoi paquet Hello vers {} voisins", self.neighbors.len());
        
        // Envoyer Hello à tous les voisins connus
        let mut tasks = Vec::new();
        
        for (neighbor_id, neighbor) in &self.neighbors {
            let serialized_clone = serialized.clone();
            let neighbor_addr = neighbor.interface_addr;
            let neighbor_id_clone = *neighbor_id;
            
            // Créer une tâche asynchrone pour chaque envoi
            let task = tokio::spawn(async move {
                match Self::send_hello_to_neighbor(neighbor_addr, serialized_clone).await {
                    Ok(()) => {
                        println!("Hello envoyé avec succès à {}", neighbor_id_clone);
                    }
                    Err(e) => {
                        log::warn!("Échec envoi Hello à {}: {}", neighbor_id_clone, e);
                    }
                }
            });
            
            tasks.push(task);
        }
        
        // Envoyer aussi en multicast sur l'interface locale (simulation)
        let multicast_task = tokio::spawn({
            let serialized_clone = serialized.clone();
            async move {
                if let Err(e) = Self::send_hello_multicast(serialized_clone).await {
                    log::warn!("Échec envoi Hello multicast: {}", e);
                } else {
                    println!("Hello multicast envoyé");
                }
            }
        });
        
        tasks.push(multicast_task);
        
        // Attendre que tous les envois se terminent
        for task in tasks {
            let _ = task.await;
        }
        
        log::info!("Cycle d'envoi Hello terminé");
        Ok(())
    }

    async fn send_hello_to_neighbor(
        neighbor_addr: Ipv4Addr,
        hello_data: String,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use tokio::net::TcpStream;
        use tokio::io::AsyncWriteExt;
        use std::time::Duration;
        
        let addr = format!("{}:8080", neighbor_addr);
        
        // Timeout pour éviter les blocages
        let connection_future = TcpStream::connect(&addr);
        let mut stream = match tokio::time::timeout(Duration::from_secs(5), connection_future).await {
            Ok(Ok(stream)) => stream,
            Ok(Err(e)) => return Err(format!("Connexion échouée vers {}: {}", addr, e).into()),
            Err(_) => return Err(format!("Timeout connexion vers {}", addr).into()),
        };
        
        // Envoyer le paquet Hello
        stream.write_all(hello_data.as_bytes()).await?;
        stream.flush().await?;
        
        // Fermer proprement la connexion
        let _ = stream.shutdown().await;
        
        Ok(())
    }
    
    // Fonction helper pour envoyer Hello en multicast (découverte de nouveaux voisins)
    async fn send_hello_multicast(
        hello_data: String,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use tokio::net::UdpSocket;
        
        // Utiliser UDP multicast pour la découverte de voisins
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        
        // Adresses de broadcast communes pour la découverte
        let broadcast_addrs = [
            "224.0.0.5:8080",      // Multicast OSPF AllSPFRouters
            "224.0.0.6:8080",      // Multicast OSPF AllDRouters
        ];
        
        for addr in &broadcast_addrs {
            match socket.send_to(hello_data.as_bytes(), addr).await {
                Ok(_) => log::trace!("Hello multicast envoyé vers {}", addr),
                Err(e) => log::trace!("Échec multicast vers {}: {}", addr, e),
            }
        }
        
        Ok(())
    }
    
    pub async fn generate_and_flood_lsa(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut lsa_links = Vec::new();
        
        // Créer les liens LSA à partir des liens actifs
        for (neighbor_id, link) in &self.links {
            if link.state == LinkState::Up {
                lsa_links.push(RouterLSALink {
                    link_id: *neighbor_id,
                    link_data: link.interface_addr,
                    link_type: 1, // Point-to-point
                    num_metrics: 0,
                    metric: link.cost,
                });
            }
        }
        
        let lsa = RouterLSA {
            header: LSAHeader {
                lsa_type: 1, // Router LSA
                link_state_id: self.config.router_id,
                advertising_router: self.config.router_id,
                sequence_number: self.sequence_number,
                age: 0,
                checksum: 0, // Simplifié
                length: 0,   // Calculé automatiquement
            },
            flags: 0,
            num_links: lsa_links.len() as u16,
            links: lsa_links,
        };
        
        // Ajouter à notre propre base de données
        self.lsa_database.insert(self.config.router_id, lsa.clone());
        self.sequence_number += 1;
        
        let message = OSPFMessage::LSA(lsa);
        let serialized = serde_json::to_string(&message)?;
        
        // TODO: Diffuser aux voisins
        println!("Génération et diffusion LSA: {}", serialized);
        
        Ok(())
    }
    
    pub async fn add_link(&mut self, neighbor_id: Ipv4Addr, link: Link) {
        self.links.insert(neighbor_id, link);
        log::info!("Lien ajouté vers {}", neighbor_id);
    }
    
    pub async fn calculate_routing_table(&mut self) {
        self.routing_table.clear();
        
        // Construire le graphe à partir de la base LSA
        let mut graph = UnGraph::new_undirected();
        let mut node_map = HashMap::new();
        
        // Ajouter tous les routeurs comme nœuds
        for router_id in self.lsa_database.keys() {
            let node = graph.add_node(*router_id);
            node_map.insert(*router_id, node);
        }
        
        // Ajouter notre propre routeur s'il n'est pas dans la base
        if !node_map.contains_key(&self.config.router_id) {
            let node = graph.add_node(self.config.router_id);
            node_map.insert(self.config.router_id, node);
        }
        
        // Ajouter les arêtes basées sur les LSA
        for (router_id, lsa) in &self.lsa_database {
            if let Some(&source_node) = node_map.get(router_id) {
                for link in &lsa.links {
                    if let Some(&target_node) = node_map.get(&link.link_id) {
                        graph.add_edge(source_node, target_node, link.metric);
                    }
                }
            }
        }
        
        // Appliquer Dijkstra depuis notre routeur
        if let Some(&start_node) = node_map.get(&self.config.router_id) {
            let distances = dijkstra(&graph, start_node, None, |e| *e.weight());
            
            // Construire la table de routage
            for (dest_router, &dest_node) in &node_map {
                if *dest_router != self.config.router_id {
                    if let Some(&cost) = distances.get(&dest_node) {
                        // Trouver le prochain saut (simplifié)
                        let next_hop = self.find_next_hop(*dest_router);
                        
                        self.routing_table.push(RoutingEntry {
                            destination: *dest_router,
                            next_hop: next_hop.unwrap_or(*dest_router),
                            cost,
                            interface: "eth0".to_string(), // Simplifié
                        });
                    }
                }
            }
        }
        
        log::info!("Table de routage recalculée avec {} entrées", self.routing_table.len());
    }
    
    fn find_next_hop(&self, destination: Ipv4Addr) -> Option<Ipv4Addr> {
        // Logique simplifiée pour trouver le prochain saut
        // Dans une implémentation complète, il faudrait tracer le chemin
        for (neighbor_id, link) in &self.links {
            if link.state == LinkState::Up {
                return Some(*neighbor_id);
            }
        }
        None
    }
    
    pub async fn get_routing_table(&self) -> &[RoutingEntry] {
        &self.routing_table
    }
    
    pub async fn cleanup_dead_neighbors(&mut self) {
        let now = current_timestamp();
        let dead_threshold = self.config.dead_interval as u64;
        
        let dead_neighbors: Vec<Ipv4Addr> = self.neighbors
            .iter()
            .filter(|(_, neighbor)| now - neighbor.last_seen > dead_threshold)
            .map(|(&id, _)| id)
            .collect();
        
        for neighbor_id in dead_neighbors {
            self.neighbors.remove(&neighbor_id);
            if let Some(link) = self.links.get_mut(&neighbor_id) {
                link.state = LinkState::Down;
            }
            log::info!("Voisin {} marqué comme mort", neighbor_id);
        }
        
        if !self.neighbors.is_empty() {
            self.calculate_routing_table().await;
        }
    }
}