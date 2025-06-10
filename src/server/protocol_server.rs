// This file implements the server-side logic for responding to client requests, including functions to manage incoming messages and send replies.

use tokio::net::UdpSocket;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use log::{info, error, debug};
use crate::protocol::message_types::*;
use crate::core::routing_table::RoutingTable;
use crate::core::neighbor_discovery::NeighborDiscovery;
use crate::error::ProtocolError;

pub struct ProtocolServer {
    socket: Arc<UdpSocket>,
    routing_table: Arc<RwLock<RoutingTable>>,
    neighbor_discovery: Arc<RwLock<NeighborDiscovery>>,
    broadcast_addr: SocketAddr,
}

impl ProtocolServer {
    pub async fn new(bind_addr: &str, broadcast_addr: &str) -> Result<Self, ProtocolError> {
        let socket = UdpSocket::bind(bind_addr).await?;
        socket.set_broadcast(true)?;
        
        info!("Protocol server bound to {}", bind_addr);
        
        Ok(Self {
            socket: Arc::new(socket),
            routing_table: Arc::new(RwLock::new(RoutingTable::new())),
            neighbor_discovery: Arc::new(RwLock::new(NeighborDiscovery::new())),
            broadcast_addr: broadcast_addr.parse()?,
        })
    }
    
    pub async fn start(&self) -> Result<(), ProtocolError> {
        info!("Starting protocol server...");
        
        // Démarrer le processus de découverte des voisins
        self.start_neighbor_discovery().await?;
        
        // Démarrer l'écoute des messages
        self.listen_for_messages().await?;
        
        Ok(())
    }
    
    async fn start_neighbor_discovery(&self) -> Result<(), ProtocolError> {
        let socket = self.socket.clone();
        let broadcast_addr = self.broadcast_addr;
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
            
            loop {
                interval.tick().await;
                
                let hello_msg = ProtocolMessage::Hello(HelloMessage {
                    router_id: "local_router".to_string(),
                    sequence_number: 1,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                });
                
                match serde_json::to_string(&hello_msg) {
                    Ok(msg_str) => {
                        if let Err(e) = socket.send_to(msg_str.as_bytes(), broadcast_addr).await {
                            error!("Failed to send hello message: {}", e);
                        } else {
                            debug!("Sent hello message via broadcast");
                        }
                    }
                    Err(e) => error!("Failed to serialize hello message: {}", e),
                }
            }
        });
        
        Ok(())
    }
    
    async fn listen_for_messages(&self) -> Result<(), ProtocolError> {
        let mut buf = [0; 1024];
        
        loop {
            match self.socket.recv_from(&mut buf).await {
                Ok((len, addr)) => {
                    let msg_str = String::from_utf8_lossy(&buf[..len]);
                    debug!("Received message from {}: {}", addr, msg_str);
                    
                    if let Ok(message) = serde_json::from_str::<ProtocolMessage>(&msg_str) {
                        self.handle_message(message, addr).await;
                    } else {
                        error!("Failed to parse message from {}", addr);
                    }
                }
                Err(e) => {
                    error!("Error receiving message: {}", e);
                    break;
                }
            }
        }
        
        Ok(())
    }
    
    async fn handle_message(&self, message: ProtocolMessage, sender: SocketAddr) {
        match message {
            ProtocolMessage::Hello(hello) => {
                info!("Received hello from router {} at {}", hello.router_id, sender);
                
                let mut neighbor_discovery = self.neighbor_discovery.write().await;
                neighbor_discovery.add_neighbor(hello.router_id.clone(), sender).await;
            }
            ProtocolMessage::LinkState(link_state) => {
                info!("Received link state update from {}", sender);
                
                let mut routing_table = self.routing_table.write().await;
                routing_table.update_from_link_state(link_state).await;
            }
            ProtocolMessage::RouteRequest(request) => {
                info!("Received route request for {} from {}", request.destination, sender);
                self.handle_route_request(request, sender).await;
            }
            ProtocolMessage::RouteResponse(response) => {
                info!("Received route response from {}", sender);
                self.handle_route_response(response).await;
            }
        }
    }
    
    async fn handle_route_request(&self, request: RouteRequest, sender: SocketAddr) {
        let routing_table = self.routing_table.read().await;
        
        if let Some(route) = routing_table.get_route(&request.destination).await {
            let response = ProtocolMessage::RouteResponse(RouteResponse {
                destination: request.destination,
                next_hop: route.next_hop,
                metric: route.metric,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            });
            
            if let Ok(msg_str) = serde_json::to_string(&response) {
                if let Err(e) = self.socket.send_to(msg_str.as_bytes(), sender).await {
                    error!("Failed to send route response: {}", e);
                }
            }
        }
    }
    
    async fn handle_route_response(&self, response: RouteResponse) {
        let mut routing_table = self.routing_table.write().await;
        routing_table.update_route(
            response.destination,
            response.next_hop,
            response.metric,
        ).await;
    }
    
    pub async fn broadcast_link_state(&self, link_state: LinkStateUpdate) -> Result<(), ProtocolError> {
        let message = ProtocolMessage::LinkState(link_state);
        let msg_str = serde_json::to_string(&message)?;
        
        self.socket.send_to(msg_str.as_bytes(), self.broadcast_addr).await?;
        info!("Broadcasted link state update");
        
        Ok(())
    }
}