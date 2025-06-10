// This file contains integration tests for the routing protocol, ensuring that different components work together as expected.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::protocol_client;
    use crate::server::protocol_server;
    use crate::core::routing_table;
    use crate::core::neighbor_discovery;
    use crate::core::path_calculation;
    use crate::core::network_interface;

    #[test]
    fn test_client_server_communication() {
        // Setup a mock server and client
        let server = protocol_server::start_server();
        let client = protocol_client::new();

        // Simulate a request from the client to the server
        let response = client.send_request("Test Request");

        // Assert that the response is as expected
        assert_eq!(response, "Expected Response");
    }

    #[test]
    fn test_routing_table_update() {
        let mut routing_table = routing_table::RoutingTable::new();
        routing_table.add_route("192.168.1.0/24", "192.168.1.1");

        // Check if the route was added correctly
        assert!(routing_table.contains("192.168.1.0/24"));

        // Update the route
        routing_table.update_route("192.168.1.0/24", "192.168.1.2");
        assert_eq!(routing_table.get_next_hop("192.168.1.0/24"), Some("192.168.1.2"));
    }

    #[test]
    fn test_neighbor_discovery() {
        let mut neighbor_discovery = neighbor_discovery::NeighborDiscovery::new();
        neighbor_discovery.add_neighbor("192.168.1.2", "RouterA");

        // Check if the neighbor was added correctly
        assert!(neighbor_discovery.is_neighbor("192.168.1.2"));
        assert_eq!(neighbor_discovery.get_neighbor_name("192.168.1.2"), Some("RouterA"));
    }

    #[test]
    fn test_path_calculation() {
        let paths = vec![
            ("192.168.1.1", "192.168.1.2", 1),
            ("192.168.1.1", "192.168.1.3", 2),
        ];
        let best_path = path_calculation::calculate_best_path(&paths);

        // Assert that the best path is correct
        assert_eq!(best_path, ("192.168.1.1", "192.168.1.2"));
    }

    #[test]
    fn test_network_interface_management() {
        let mut network_interface = network_interface::NetworkInterface::new();
        network_interface.add_interface("eth0");

        // Check if the interface was added correctly
        assert!(network_interface.is_interface_active("eth0"));
    }
}