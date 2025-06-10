mod neighbor_discovery {
    use std::collections::HashMap;
    use std::net::SocketAddr;
    use crate::protocol::message_types::NeighborInfo;
    use crate::error::ProtocolError;

    #[derive(Debug, Clone)]
    pub struct Neighbor {
        pub ip_address: IpAddr,
        pub system_name: String,
    }

    pub struct NeighborDiscovery {
        neighbors: HashMap<String, NeighborInfo>,
    }

    impl NeighborDiscovery {
        pub fn new() -> Self {
            Self {
                neighbors: HashMap::new(),
            }
        }
        
        pub async fn add_neighbor(&mut self, router_id: String, addr: SocketAddr) {
            let neighbor = NeighborInfo {
                router_id: router_id.clone(),
                ip_address: addr.to_string(),
                last_seen: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                is_active: true,
            };
            
            self.neighbors.insert(router_id, neighbor);
        }
        
        pub async fn get_neighbor(&self, router_id: &str) -> Option<&NeighborInfo> {
            self.neighbors.get(router_id)
        }
        
        pub fn get_all_neighbors(&self) -> &HashMap<String, NeighborInfo> {
            &self.neighbors
        }
        
        pub async fn update_neighbor_status(&mut self, router_id: &str, is_active: bool) {
            if let Some(neighbor) = self.neighbors.get_mut(router_id) {
                neighbor.is_active = is_active;
                neighbor.last_seen = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
            }
        }
        
        pub async fn cleanup_inactive_neighbors(&mut self, timeout_seconds: u64) {
            let current_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
                
            self.neighbors.retain(|_, neighbor| {
                current_time - neighbor.last_seen < timeout_seconds
            });
        }
    }
}