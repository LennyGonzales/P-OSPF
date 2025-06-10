// This file serves as the core module, organizing essential functionalities related to routing and network management.

pub mod routing_table;
pub mod neighbor_discovery;
pub mod network_interface;

pub use routing_table::RoutingTable;
pub use neighbor_discovery::NeighborDiscovery;
pub use network_interface::NetworkInterface;