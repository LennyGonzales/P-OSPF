# Rust Routing Protocol

This project implements a dynamic routing protocol in Rust, designed to replace OSPF for local networks. The protocol is simple, robust, secure, and resource-efficient, specifically tailored for IPv4 networks.

## Features

- **Dynamic Routing**: Automatically calculates the best paths between routers based on the shortest path algorithm, link states, and capacities.
- **Neighbor Discovery**: Maintains a list of neighboring routers with their IP addresses and system names.
- **Routing Table Management**: Updates the routing table based on the calculated best paths and network changes.
- **Interface Management**: Allows specification of network interfaces to be included in routing calculations.
- **Fault Tolerance**: Capable of tolerating network failures and adjusting routes accordingly.
- **Client-Server Architecture**: The protocol is divided into client and server components for efficient communication.

## Project Structure

- `src/main.rs`: Entry point of the application, initializing the routing protocol and managing execution flow.
- `src/lib.rs`: Library interface for the routing protocol.
- `src/client/`: Contains client-side logic for emitting requests and handling responses.
- `src/server/`: Implements server-side logic for responding to client requests.
- `src/core/`: Core functionalities related to routing and network management.
- `src/protocol/`: Protocol definitions, including message types and packet handling.
- `src/utils/`: Utility functions for logging and configuration management.
- `src/error.rs`: Custom error types and handling mechanisms.
- `config/default.toml`: Default configuration settings for the routing protocol.
- `tests/`: Contains integration and unit tests to ensure functionality.

## Setup Instructions

1. Clone the repository:
   ```
   git clone <repository-url>
   cd rust-routing-protocol
   ```

2. Build the project:
   ```
   cargo build
   ```

3. Run the application:
   ```
   cargo run
   ```

## Usage

- The protocol can be activated or deactivated on demand.
- Each router can specify which network interfaces to include in the routing calculations.
- The list of neighboring routers can be displayed upon request.

## Contributing

Contributions are welcome! Please submit a pull request or open an issue for any enhancements or bug fixes.

## License

This project is licensed under the MIT License. See the LICENSE file for details.