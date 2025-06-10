// This file implements the server-side logic for responding to client requests, including functions to manage incoming messages and send replies.

use std::net::{UdpSocket, SocketAddr};
use std::collections::HashMap;
use std::str;
use crate::protocol::message_types::{Message, Response};
use crate::core::routing_table::RoutingTable;
use crate::core::neighbor_discovery::NeighborDiscovery;

pub struct ProtocolServer {
    socket: UdpSocket,
    routing_table: RoutingTable,
    neighbors: NeighborDiscovery,
}

impl ProtocolServer {
    pub fn new(bind_addr: &str) -> std::io::Result<Self> {
        let socket = UdpSocket::bind(bind_addr)?;
        let routing_table = RoutingTable::new();
        let neighbors = NeighborDiscovery::new();
        Ok(ProtocolServer { socket, routing_table, neighbors })
    }

    pub fn run(&mut self) -> std::io::Result<()> {
        let mut buf = [0; 1024];
        loop {
            let (size, src) = self.socket.recv_from(&mut buf)?;
            self.handle_request(&buf[..size], src)?;
        }
    }

    fn handle_request(&mut self, buf: &[u8], src: SocketAddr) -> std::io::Result<()> {
        let message: Message = bincode::deserialize(buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        match message {
            Message::Hello => {
                self.neighbors.add_neighbor(src);
                self.send_response(src, Response::HelloAck)?;
            }
            // Handle other message types here
            _ => {}
        }
        Ok(())
    }

    fn send_response(&self, addr: SocketAddr, response: Response) -> std::io::Result<()> {
        let encoded: Vec<u8> = bincode::serialize(&response).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        self.socket.send_to(&encoded, addr)?;
        Ok(())
    }
}