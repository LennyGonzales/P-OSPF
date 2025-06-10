// This file implements the routing table structure, including functions to add, remove, and update routes based on the calculated best paths.

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Route {
    pub destination: String,
    pub next_hop: String,
    pub metric: u32,
}

#[derive(Debug, Default)]
pub struct RoutingTable {
    routes: HashMap<String, Route>,
}

impl RoutingTable {
    pub fn new() -> Self {
        RoutingTable {
            routes: HashMap::new(),
        }
    }

    pub fn add_route(&mut self, destination: String, next_hop: String, metric: u32) {
        let route = Route {
            destination,
            next_hop,
            metric,
        };
        self.routes.insert(route.destination.clone(), route);
    }

    pub fn remove_route(&mut self, destination: &str) {
        self.routes.remove(destination);
    }

    pub fn update_route(&mut self, destination: String, next_hop: String, metric: u32) {
        if let Some(route) = self.routes.get_mut(&destination) {
            route.next_hop = next_hop;
            route.metric = metric;
        }
    }

    pub fn get_route(&self, destination: &str) -> Option<&Route> {
        self.routes.get(destination)
    }

    pub fn list_routes(&self) -> Vec<&Route> {
        self.routes.values().collect()
    }
}