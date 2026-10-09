//! Local intercept proxy with an encrypted frame channel and pluggable
//! outbound transports. The P2P transport is a stub behind the same
//! interface — when it lands, nothing above it changes.

pub mod channel;
pub mod server;
pub mod transport;

pub use channel::{Frame, SessionCipher};
pub use server::{spawn_proxy, ProxyConfig, ProxyServer, ProxySnapshot};
pub use transport::{Transport, TransportMode};
