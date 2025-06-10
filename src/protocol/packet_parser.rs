mod packet_parser {
    use std::net::Ipv4Addr;

    #[derive(Debug)]
    pub struct Packet {
        pub source_ip: Ipv4Addr,
        pub destination_ip: Ipv4Addr,
        pub payload: Vec<u8>,
    }

    impl Packet {
        pub fn new(source_ip: Ipv4Addr, destination_ip: Ipv4Addr, payload: Vec<u8>) -> Self {
            Packet {
                source_ip,
                destination_ip,
                payload,
            }
        }
    }

    pub fn parse_packet(data: &[u8]) -> Option<Packet> {
        if data.len() < 8 {
            return None; // Not enough data to form a packet
        }

        let source_ip = Ipv4Addr::from([data[0], data[1], data[2], data[3]]);
        let destination_ip = Ipv4Addr::from([data[4], data[5], data[6], data[7]]);
        let payload = data[8..].to_vec();

        Some(Packet::new(source_ip, destination_ip, payload))
    }

    pub fn serialize_packet(packet: &Packet) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&packet.source_ip.octets());
        data.extend_from_slice(&packet.destination_ip.octets());
        data.extend_from_slice(&packet.payload);
        data
    }
}

use serde_json;
use crate::protocol::message_types::ProtocolMessage;
use crate::error::ProtocolError;

pub struct PacketParser;

impl PacketParser {
    pub fn parse_message(data: &[u8]) -> Result<ProtocolMessage, ProtocolError> {
        let json_str = std::str::from_utf8(data)
            .map_err(|_| ProtocolError::Protocol("Invalid UTF-8 in packet".to_string()))?;
            
        let message: ProtocolMessage = serde_json::from_str(json_str)?;
        Ok(message)
    }
    
    pub fn serialize_message(message: &ProtocolMessage) -> Result<Vec<u8>, ProtocolError> {
        let json_str = serde_json::to_string(message)?;
        Ok(json_str.into_bytes())
    }
    
    pub fn validate_message(message: &ProtocolMessage) -> bool {
        match message {
            ProtocolMessage::Hello(hello) => {
                !hello.router_id.is_empty() && hello.timestamp > 0
            }
            ProtocolMessage::LinkState(link_state) => {
                !link_state.router_id.is_empty() && link_state.timestamp > 0
            }
            ProtocolMessage::RouteRequest(request) => {
                !request.destination.is_empty() && !request.source.is_empty()
            }
            ProtocolMessage::RouteResponse(response) => {
                !response.destination.is_empty() && !response.next_hop.is_empty()
            }
        }
    }
}