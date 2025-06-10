mod request_handler {
    use crate::protocol::message_types::{Request, Response};
    use crate::server::response_handler::handle_response;
    use std::net::UdpSocket;
    use tokio::net::UdpSocket as TokioUdpSocket;
    use std::sync::Arc;
    use crate::error::ProtocolError;

    pub struct RequestHandler {
        socket: Arc<TokioUdpSocket>,
    }

    impl RequestHandler {
        pub fn new(socket: Arc<TokioUdpSocket>) -> Self {
            Self { socket }
        }
        
        pub async fn handle_response(&self) -> Result<ProtocolMessage, ProtocolError> {
            let mut buf = [0; 1024];
            let (len, _) = self.socket.recv_from(&mut buf).await?;
            
            let msg_str = String::from_utf8_lossy(&buf[..len]);
            let message: ProtocolMessage = serde_json::from_str(&msg_str)?;
            
            Ok(message)
        }
    }

    pub fn handle_request(socket: &UdpSocket, buf: &[u8]) {
        match bincode::deserialize::<Request>(buf) {
            Ok(request) => {
                // Process the request and generate a response
                let response = process_request(request);
                // Send the response back to the client
                if let Err(e) = socket.send_to(&bincode::serialize(&response).unwrap(), request.source) {
                    eprintln!("Failed to send response: {}", e);
                }
            }
            Err(e) => {
                eprintln!("Failed to deserialize request: {}", e);
            }
        }
    }

    fn process_request(request: Request) -> Response {
        // Implement the logic to process the request and generate a response
        // This is a placeholder implementation
        Response {
            status: "OK".to_string(),
            data: None,
        }
    }
}