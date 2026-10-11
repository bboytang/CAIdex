//! Official account authentication. Account tokens confer no Host authority.
mod client;
mod store;
pub use client::*;
pub use store::*;
#[cfg(feature = "server")]
pub mod server;
