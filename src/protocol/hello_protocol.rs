mod hello_protocol {
    use std::net::IpAddr;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::error::ProtocolError;
    use crate::protocol::message_types::{HelloMessage, ProtocolMessage};

    #[derive(Debug)]
    pub struct HelloMessage {
        pub source_ip: IpAddr,
        pub neighbor_ip: IpAddr,
        pub sequence_number: u32,
        pub router_id: String,
        pub timestamp: u64,
    }

    impl HelloMessage {
        pub fn new(
            source_ip: IpAddr,
            neighbor_ip: IpAddr,
            sequence_number: u32,
            router_id: String,
        ) -> Self {
            HelloMessage {
                source_ip,
                neighbor_ip,
                sequence_number,
                router_id,
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            }
        }

        pub fn serialize(&self) -> Vec<u8> {
            let mut buffer = Vec::new();
            buffer.extend_from_slice(&self.source_ip.octets());
            buffer.extend_from_slice(&self.neighbor_ip.octets());
            buffer.extend_from_slice(&self.sequence_number.to_be_bytes());
            buffer.extend_from_slice(self.router_id.as_bytes());
            buffer.extend_from_slice(&self.timestamp.to_be_bytes());
            buffer
        }

        pub fn deserialize(data: &[u8]) -> Option<Self> {
            if data.len() < 12 {
                return None;
            }
            let source_ip = IpAddr::from(<[u8; 4]>::try_from(&data[0..4]).ok()?);
            let neighbor_ip = IpAddr::from(<[u8; 4]>::try_from(&data[4..8]).ok()?);
            let sequence_number = u32::from_be_bytes(data[8..12].try_into().ok()?);
            let router_id = String::from_utf8_lossy(&data[12..]).to_string();
            let timestamp = u64::from_be_bytes(data[data.len() - 8..data.len()].try_into().ok()?);
            Some(HelloMessage {
                source_ip,
                neighbor_ip,
                sequence_number,
                router_id,
                timestamp,
            })
        }
    }

    pub struct HelloProtocol {
        router_id: String,
        sequence_number: u32,
        hello_interval: u64,
    }

    impl HelloProtocol {
        pub fn new(router_id: String, hello_interval: u64) -> Self {
            Self {
                router_id,
                sequence_number: 0,
                hello_interval,
            }
        }

        pub fn create_hello_message(&mut self) -> Result<ProtocolMessage, ProtocolError> {
            self.sequence_number += 1;

            let hello = HelloMessage {
                router_id: self.router_id.clone(),
                sequence_number: self.sequence_number,
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                ..Default::default()
            };

            Ok(ProtocolMessage::Hello(hello))
        }

        pub fn get_hello_interval(&self) -> u64 {
            self.hello_interval
        }

        pub fn set_hello_interval(&mut self, interval: u64) {
            self.hello_interval = interval;
        }

        pub fn validate_hello(&self, hello: &HelloMessage) -> bool {
            !hello.router_id.is_empty()
                && hello.timestamp > 0
                && hello.router_id != self.router_id
        }
    }

    pub fn send_hello_message(message: &HelloMessage) {
        // Logic to send the hello message over the network
    }

    pub fn receive_hello_message(data: &[u8]) -> Option<HelloMessage> {
        HelloMessage::deserialize(data)
    }
}