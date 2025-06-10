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