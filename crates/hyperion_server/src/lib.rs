//! Hyperion server library.
//!
//! All server logic lives here so it can be integration-tested from
//! [`tests/`](crate) and reused by other binaries (`tools/packet_inspector`,
//! admin CLIs, …). The `hyperion-server` binary in `main.rs` is a thin
//! wrapper around [`network::serve`].

pub mod config;
pub mod key_pool;
pub mod network;
pub mod session;
