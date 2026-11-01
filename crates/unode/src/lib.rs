//! Networked pieces shared by the node binaries: JSON-RPC client, senders, config.
pub mod rpc;
pub mod sender;
#[cfg(feature = "grpc")]
pub mod grpc;
