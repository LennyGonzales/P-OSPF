mod neighbor_discovery {
    use std::collections::HashMap;
    use std::net::IpAddr;

    #[derive(Debug, Clone)]
    pub struct Neighbor {
        pub ip_address: IpAddr,
        pub system_name: String,
    }

    pub struct NeighborDiscovery {
        neighbors: HashMap<IpAddr, Neighbor>,
    }

    impl NeighborDiscovery {
        pub fn new() -> Self {
            NeighborDiscovery {
                neighbors: HashMap::new(),
            }
        }

        pub fn add_neighbor(&mut self, ip_address: IpAddr, system_name: String) {
            let neighbor = Neighbor {
                ip_address,
                system_name,
            };
            self.neighbors.insert(ip_address, neighbor);
        }

        pub fn remove_neighbor(&mut self, ip_address: &IpAddr) {
            self.neighbors.remove(ip_address);
        }

        pub fn get_neighbors(&self) -> Vec<&Neighbor> {
            self.neighbors.values().collect()
        }

        pub fn display_neighbors(&self) {
            for neighbor in self.get_neighbors() {
                println!("Neighbor IP: {}, System Name: {}", neighbor.ip_address, neighbor.system_name);
            }
        }
    }
}