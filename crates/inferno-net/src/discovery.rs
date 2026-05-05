//! mDNS-based Dante service discovery.
//!
//! Dante advertises four service types over mDNS:
//!   - `_netaudio-arc._udp.local.`  (audio routing control)
//!   - `_netaudio-cmc._udp.local.`  (control & monitoring)
//!   - `_netaudio-dbc._udp.local.`  (device-to-device)
//!   - `_netaudio-chan._udp.local.` (channel arc)

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Duration;

use mdns_sd::{ServiceDaemon, ServiceEvent};
use tracing::{debug, warn};

use crate::device::{DanteService, Device, ServiceKind};
use crate::error::{Error, Result};

pub const SERVICE_ARC: &str = "_netaudio-arc._udp.local.";
pub const SERVICE_CMC: &str = "_netaudio-cmc._udp.local.";
pub const SERVICE_DBC: &str = "_netaudio-dbc._udp.local.";
pub const SERVICE_CHAN: &str = "_netaudio-chan._udp.local.";

pub const ALL_SERVICES: &[(&str, ServiceKind)] = &[
    (SERVICE_ARC, ServiceKind::Arc),
    (SERVICE_CMC, ServiceKind::Cmc),
    (SERVICE_DBC, ServiceKind::Dbc),
    (SERVICE_CHAN, ServiceKind::ChanArc),
];

/// Browse for Dante devices for `duration`. Returns a map of device-name → Device.
pub async fn browse(duration: Duration) -> Result<HashMap<String, Device>> {
    let daemon = ServiceDaemon::new().map_err(|e| Error::Mdns(e.to_string()))?;
    let mut devices: HashMap<String, Device> = HashMap::new();

    // Browse all four services. Chan records publish one entry per channel
    // (`<chan>@<device>._netaudio-chan._udp.local.`); we use them to pre-
    // populate `Device::mdns_channel_names` without touching ARC.
    let mut receivers = Vec::new();
    for (svc, kind) in ALL_SERVICES {
        let rx = daemon.browse(svc).map_err(|e| Error::Mdns(e.to_string()))?;
        receivers.push((*svc, *kind, rx));
    }

    let deadline = tokio::time::Instant::now() + duration;
    loop {
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        for (_svc, kind, rx) in &receivers {
            while let Ok(event) = rx.try_recv() {
                if let ServiceEvent::ServiceResolved(info) = event {
                    let kind = *kind;
                    let fullname = info.get_fullname().to_string();
                    // First label: either `<device>` (arc/cmc/dbc) or
                    // `<chan>@<device>` (chan-arc). Split off the parent
                    // device name in both cases.
                    let first_label = fullname
                        .split_once('.')
                        .map(|(n, _)| n.to_string())
                        .unwrap_or_else(|| fullname.clone());
                    let (device_name, chan_name) = match (kind, first_label.split_once('@')) {
                        (ServiceKind::ChanArc, Some((chan, dev))) => {
                            (dev.to_string(), Some(chan.to_string()))
                        }
                        _ => (first_label, None),
                    };

                    let entry = devices
                        .entry(device_name.clone())
                        .or_insert_with(|| Device {
                            name: device_name.clone(),
                            manufacturer: None,
                            model: None,
                            addresses: Vec::new(),
                            services: Vec::new(),
                            tx_channels: Vec::new(),
                            rx_channels: Vec::new(),
                            subscriptions: Vec::new(),
                            sample_rate: None,
                            latency_ns: None,
                            mdns_channel_names: Vec::new(),
                        });

                    for ip in info.get_addresses() {
                        let ip: IpAddr = *ip;
                        if !entry.addresses.contains(&ip) {
                            entry.addresses.push(ip);
                        }
                    }
                    if let Some(chan) = chan_name {
                        if !entry.mdns_channel_names.contains(&chan) {
                            entry.mdns_channel_names.push(chan);
                        }
                    } else {
                        entry.services.push(DanteService {
                            kind,
                            port: info.get_port(),
                            host: info.get_hostname().to_string(),
                        });
                    }
                    debug!(device = %device_name, ?kind, "resolved");
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    if let Err(e) = daemon.shutdown() {
        warn!("mdns shutdown: {e}");
    }
    Ok(devices)
}
