//! inferno-net: Dante network audio control protocol.

pub mod aliases;
pub mod device;
pub mod discovery;
pub mod error;
pub mod mcast;
pub mod metering;
pub mod protocol;

pub use device::{Channel, Device, Direction, Subscription};
pub use error::{Error, Result};
pub use protocol::DanteClient;
