// This file defines the library interface for the routing protocol, exporting necessary modules and types for use in other parts of the application.

pub mod client;
pub mod server;
pub mod core;
pub mod protocol;
pub mod utils;
pub mod error;

pub use error::ProtocolError;