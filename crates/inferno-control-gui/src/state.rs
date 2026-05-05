use std::net::IpAddr;

use inferno_net::protocol::parser::{RxChannel, TxChannel};

#[derive(Clone, Default, PartialEq)]
pub struct AppState {
    pub devices: Vec<DeviceEntry>,
    pub scanning: bool,
    pub tx_filter: String,
    pub rx_filter: String,
    pub status: Option<Status>,
}

#[derive(Clone, PartialEq)]
pub struct DeviceEntry {
    pub name: String,
    pub ip: IpAddr,
    pub model: Option<String>,
    pub sample_rate: Option<u32>,
    pub latency_us: Option<u32>,
    pub rx_channels: Vec<RxChannel>,
    pub tx_channels: Vec<TxChannel>,
    pub rx_collapsed: bool,
    pub tx_collapsed: bool,
}

#[derive(Clone, PartialEq)]
pub struct Status {
    pub message: String,
    pub error: bool,
}

impl Status {
    pub fn ok(msg: &str) -> Self {
        Self { message: msg.into(), error: false }
    }
    pub fn err(msg: &str) -> Self {
        Self { message: msg.into(), error: true }
    }
}

impl DeviceEntry {
    pub fn visible_tx<'a>(&'a self, filter: &'a str) -> impl Iterator<Item = &'a TxChannel> + 'a {
        let f = filter.to_ascii_lowercase();
        self.tx_channels.iter().filter(move |c| {
            f.is_empty()
                || c.name.to_ascii_lowercase().contains(&f)
                || self.name.to_ascii_lowercase().contains(&f)
        })
    }
    pub fn visible_rx<'a>(&'a self, filter: &'a str) -> impl Iterator<Item = &'a RxChannel> + 'a {
        let f = filter.to_ascii_lowercase();
        self.rx_channels.iter().filter(move |c| {
            f.is_empty()
                || c.name.to_ascii_lowercase().contains(&f)
                || self.name.to_ascii_lowercase().contains(&f)
        })
    }
}
