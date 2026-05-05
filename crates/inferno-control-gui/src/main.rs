//! Inferno Control — Dante Controller-style routing matrix.
//!
//! Layout (mirrors the Audinate Dante Controller):
//!   - top header row    : transmitter (TX) channels grouped by device
//!   - left header column: receiver (RX) channels grouped by device
//!   - grid              : one cell per (RX, TX) pair; green check = active
//!                         subscription. Click a cell to (un)subscribe.
//!
//! Filter inputs above the headers narrow the visible TX / RX sets.
//! Device groups are collapsible. The current-state model is rebuilt by
//! re-scanning; subscription mutations re-fetch the affected device only.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use dioxus::prelude::*;
use inferno_net::protocol::{
    parser::{RxChannel, TxChannel},
    DanteClient, DEVICE_ARC_PORT,
};

mod state;
mod ui;

use state::{AppState, DeviceEntry, Status};

const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

fn main() {
    tracing_subscriber::fmt::init();
    dioxus::launch(app);
}

fn app() -> Element {
    let state = use_signal(AppState::default);

    rsx! {
        document::Stylesheet { href: TAILWIND_CSS }
        ui::AppShell { state }
    }
}

// --- async data layer ------------------------------------------------------

pub(crate) async fn scan_network() -> Vec<DeviceEntry> {
    let timeout = Duration::from_secs(3);
    let map = match inferno_net::discovery::browse(timeout).await {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };
    let mut entries = Vec::new();
    for (_, dev) in map {
        let Some(ip) = dev.addresses.iter().find(|a| a.is_ipv4()).copied() else {
            continue;
        };
        let entry = fetch_device(dev.name.clone(), ip).await;
        entries.push(entry);
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

pub(crate) async fn fetch_device(name: String, ip: IpAddr) -> DeviceEntry {
    let client = DanteClient::new(SocketAddr::new(ip, DEVICE_ARC_PORT), Duration::from_secs(2));
    let rx = client.all_rx_channels().await.unwrap_or_default();
    let tx = client.all_tx_channels().await.unwrap_or_default();
    let info = client.device_info().await.ok();
    let settings = client.device_settings().await.ok();
    DeviceEntry {
        name,
        ip,
        model: info.map(|i| i.model_name).filter(|s| !s.is_empty()),
        sample_rate: settings.as_ref().and_then(|s| s.sample_rate),
        latency_us: settings.as_ref().and_then(|s| s.latency_us),
        rx_channels: rx,
        tx_channels: tx,
        rx_collapsed: false,
        tx_collapsed: false,
    }
}

pub(crate) async fn toggle_subscription(
    rx_device_ip: IpAddr,
    rx_channel: RxChannel,
    tx_device_name: &str,
    tx_channel: &TxChannel,
) -> Status {
    let client = DanteClient::new(
        SocketAddr::new(rx_device_ip, DEVICE_ARC_PORT),
        Duration::from_secs(2),
    );
    let already = matches!(
        (&rx_channel.tx_device_name, &rx_channel.tx_channel_name),
        (Some(d), Some(t))
            if d == tx_device_name && t == &tx_channel.name
    );
    let result = if already {
        client.remove_subscription(rx_channel.number as u32).await
    } else {
        client
            .add_subscription(rx_channel.number, &tx_channel.name, tx_device_name)
            .await
    };
    match result {
        Ok(()) => Status::ok(if already { "unsubscribed" } else { "subscribed" }),
        Err(e) => Status::err(&format!("{e}")),
    }
}
