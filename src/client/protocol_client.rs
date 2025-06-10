// This file implements the client-side logic for emitting requests to the server,
// including functions to initiate communication and handle responses.

use std::net::{UdpSocket, SocketAddr};
use std::time::Duration;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct Request {
    pub request_type: String,
    pub payload: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Response {
    pub status: String,
    pub data: String,
}

pub struct ProtocolClient {
    socket: UdpSocket,
    server_addr: SocketAddr,
}

impl ProtocolClient {
    pub fn new(server_ip: &str, server_port: u16) -> std::io::Result<Self> {
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.set_read_timeout(Some(Duration::new(5, 0)))?;
        let server_addr = SocketAddr::new(server_ip.parse().unwrap(), server_port);
        Ok(ProtocolClient { socket, server_addr })
    }

    pub fn send_request(&self, request: Request) -> std::io::Result<Response> {
        let serialized_request = serde_json::to_string(&request).unwrap();
        self.socket.send_to(serialized_request.as_bytes(), &self.server_addr)?;

        let mut buf = [0; 1024];
        let (size, _) = self.socket.recv_from(&mut buf)?;
        let response: Response = serde_json::from_slice(&buf[..size]).unwrap();
        Ok(response)
    }
}