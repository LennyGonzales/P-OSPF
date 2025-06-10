mod hello_protocol {
    use std::net::IpAddr;

    #[derive(Debug)]
    pub struct HelloMessage {
        pub source_ip: IpAddr,
        pub neighbor_ip: IpAddr,
        pub sequence_number: u32,
    }

    impl HelloMessage {
        pub fn new(source_ip: IpAddr, neighbor_ip: IpAddr, sequence_number: u32) -> Self {
            HelloMessage {
                source_ip,
                neighbor_ip,
                sequence_number,
            }
        }

        pub fn serialize(&self) -> Vec<u8> {
            let mut buffer = Vec::new();
            buffer.extend_from_slice(&self.source_ip.octets());
            buffer.extend_from_slice(&self.neighbor_ip.octets());
            buffer.extend_from_slice(&self.sequence_number.to_be_bytes());
            buffer
        }

        pub fn deserialize(data: &[u8]) -> Option<Self> {
            if data.len() < 12 {
                return None;
            }
            let source_ip = IpAddr::from(<[u8; 4]>::try_from(&data[0..4]).ok()?);
            let neighbor_ip = IpAddr::from(<[u8; 4]>::try_from(&data[4..8]).ok()?);
            let sequence_number = u32::from_be_bytes(data[8..12].try_into().ok()?);
            Some(HelloMessage {
                source_ip,
                neighbor_ip,
                sequence_number,
            })
        }
    }

    pub fn send_hello_message(message: &HelloMessage) {
        // Logic to send the hello message over the network
    }

    pub fn receive_hello_message(data: &[u8]) -> Option<HelloMessage> {
        HelloMessage::deserialize(data)
    }
}