use std::net::IpAddr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub name: String,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub addresses: Vec<IpAddr>,
    pub services: Vec<DanteService>,
    pub tx_channels: Vec<Channel>,
    pub rx_channels: Vec<Channel>,
    pub subscriptions: Vec<Subscription>,
    pub sample_rate: Option<u32>,
    pub latency_ns: Option<u32>,
    /// Channel names harvested from `_netaudio-chan._udp` mDNS records.
    /// Populated before ARC queries complete; the GUI can render labels
    /// without waiting on a round-trip. Names are in advertisement order;
    /// duplicates are de-duped.
    #[serde(default)]
    pub mdns_channel_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DanteService {
    pub kind: ServiceKind,
    pub port: u16,
    pub host: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceKind {
    Arc,
    Cmc,
    Dbc,
    ChanArc,
}

impl Device {
    /// Returns the ARC port advertised over mDNS, falling back to the
    /// standard `4440`. Some Inferno-AoIP devices use non-standard ports
    /// (e.g. 4410), so always prefer what the device announced.
    pub fn arc_port(&self) -> u16 {
        self.services
            .iter()
            .find(|s| s.kind == ServiceKind::Arc)
            .map(|s| s.port)
            .unwrap_or(crate::protocol::DEVICE_ARC_PORT)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub number: u16,
    pub name: String,
    pub direction: Direction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Tx,
    Rx,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub rx_channel: String,
    pub rx_device: String,
    pub tx_channel: Option<String>,
    pub tx_device: Option<String>,
    pub status: Option<String>,
}
