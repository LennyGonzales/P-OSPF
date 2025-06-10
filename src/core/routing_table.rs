// This file implements the routing table structure, including functions to add, remove, and update routes based on the calculated best paths.

use std::collections::HashMap;
use std::net::SocketAddr;
use crate::protocol::message_types::LinkStateUpdate;
use crate::error::ProtocolError;

#[derive(Debug, Clone)]
pub struct Route {
    pub destination: String,
    pub next_hop: String,
    pub metric: u32,
    pub timestamp: u64,
}

pub struct RoutingTable {
    routes: HashMap<String, Route>,
}

impl RoutingTable {
    pub fn new() -> Self {
        Self {
            routes: HashMap::new(),
        }
    }
    
    pub async fn get_route(&self, destination: &str) -> Option<&Route> {
        self.routes.get(destination)
    }
    
    pub async fn update_route(&mut self, destination: String, next_hop: String, metric: u32) {
        let route = Route {
            destination: destination.clone(),
            next_hop,
            metric,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };
        
        self.routes.insert(destination, route);
    }
    
    pub async fn update_from_link_state(&mut self, link_state: LinkStateUpdate) {
        for link in link_state.links {
            if link.is_active {
                self.update_route(
                    link.neighbor_id.clone(),
                    link.neighbor_id,
                    link.metric,
                ).await;
            }
        }
    }
    
    pub fn get_all_routes(&self) -> &HashMap<String, Route> {
        &self.routes
    }
    
    pub async fn remove_route(&mut self, destination: &str) -> Option<Route> {
        self.routes.remove(destination)
    }
}