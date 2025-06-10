use crate::protocol::message_types::*;
use crate::error::ProtocolError;
use log::{debug, error, info};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;

mod response_handler {
    use super::*;

    pub struct ResponseHandler {
        socket: Arc<UdpSocket>,
    }

    impl ResponseHandler {
        pub fn new(socket: Arc<UdpSocket>) -> Self {
            Self { socket }
        }

        pub async fn send_hello_response(
            &self,
            target: SocketAddr,
            router_id: String,
        ) -> Result<(), ProtocolError> {
            let hello_msg = ProtocolMessage::Hello(HelloMessage {
                router_id,
                sequence_number: 1,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            });

            let msg_str = serde_json::to_string(&hello_msg)?;
            self.socket.send_to(msg_str.as_bytes(), target).await?;

            debug!("Sent hello response to {}", target);
            Ok(())
        }

        pub async fn send_route_response(
            &self,
            target: SocketAddr,
            destination: String,
            next_hop: String,
            metric: u32,
        ) -> Result<(), ProtocolError> {
            let response = ProtocolMessage::RouteResponse(RouteResponse {
                destination,
                next_hop,
                metric,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            });

            let msg_str = serde_json::to_string(&response)?;
            self.socket.send_to(msg_str.as_bytes(), target).await?;

            debug!("Sent route response to {}", target);
            Ok(())
        }

        pub async fn broadcast_link_state_update(
            &self,
            broadcast_addr: SocketAddr,
            link_state: LinkStateUpdate,
        ) -> Result<(), ProtocolError> {
            let message = ProtocolMessage::LinkState(link_state);
            let msg_str = serde_json::to_string(&message)?;

            self.socket.send_to(msg_str.as_bytes(), broadcast_addr).await?;
            info!("Broadcasted link state update");

            Ok(())
        }

        pub async fn send_error_response(
            &self,
            target: SocketAddr,
            error_msg: String,
        ) -> Result<(), ProtocolError> {
            let error_response = format!("{{\"error\": \"{}\"}}", error_msg);
            self.socket.send_to(error_response.as_bytes(), target).await?;

            debug!("Sent error response to {}: {}", target, error_msg);
            Ok(())
        }
    }

    pub fn handle_response(
        response: ResponseMessage,
        routing_table: &mut RoutingTable,
        neighbor_discovery: &mut NeighborDiscovery,
    ) {
        match response {
            ResponseMessage::UpdateRoutingTable(routes) => {
                for route in routes {
                    routing_table.update_route(route);
                }
                log_event("Routing table updated with new routes.");
            }
            ResponseMessage::NeighborList(neighbors) => {
                neighbor_discovery.update_neighbors(neighbors);
                log_event("Neighbor list updated.");
            }
            ResponseMessage::Error(error_msg) => {
                log_event(&format!("Error received: {}", error_msg));
            }
        }
    }

    pub fn send_response(request: &RequestMessage) -> ResponseMessage {
        // Logic to create a response based on the request
        ResponseMessage::Acknowledgment
    }
}