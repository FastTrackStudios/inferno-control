//! Dante channel-level metering.
//!
//! Devices that have been told to start metering (via the CMC port —
//! see `start_metering` builder in this module) stream UDP datagrams to
//! `224.0.0.231:<metering_port>` carrying TX + RX channel levels as
//! single-byte values. Real Dante hardware streams; the inferno-aoip
//! Rust device-side smuggles its peaks into the heartbeat instead and
//! does *not* publish to this multicast group, so use a real device for
//! validation.
//!
//! Mirrors `netaudio/dante/metering.py::parse_metering_levels`.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::trace;

use crate::error::{Error, Result};
use crate::mcast::{MulticastListener, DEFAULT_METERING_PORT, MULTICAST_GROUP_CONTROL_MONITORING};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Levels {
    /// channel number (1-based) → level (raw 0..255).
    pub tx: BTreeMap<u16, u8>,
    pub rx: BTreeMap<u16, u8>,
}

/// Parse a metering datagram. Two layouts in the wild:
///
/// 1. **Long form**: bytes `[25] = tx_count`, `[26] = rx_count`,
///    `[27..27+tx_count]` = tx levels, then `rx_count` rx bytes.
/// 2. **Tail form**: the trailing `tx_count + rx_count` bytes are
///    tx-then-rx levels, regardless of header length.
///
/// We try the long form first; if the marker bytes don't match, we
/// fall back to parsing the tail. Returns `None` if the datagram is
/// shorter than `tx_count + rx_count` (truncated or wrong device).
pub fn parse_metering_levels(data: &[u8], tx_count: u16, rx_count: u16) -> Option<Levels> {
    if tx_count == 0 && rx_count == 0 {
        return Some(Levels::default());
    }
    let total = tx_count as usize + rx_count as usize;
    if data.len() < total {
        return None;
    }
    let mut offset = data.len() - total;
    if offset >= 27 && data[25] as u16 == tx_count && data[26] as u16 == rx_count {
        offset = 27;
    }
    let tx_bytes = &data[offset..offset + tx_count as usize];
    let rx_bytes = &data[offset + tx_count as usize..offset + total];
    let mut tx = BTreeMap::new();
    for (i, &v) in tx_bytes.iter().enumerate() {
        tx.insert((i + 1) as u16, v);
    }
    let mut rx = BTreeMap::new();
    for (i, &v) in rx_bytes.iter().enumerate() {
        rx.insert((i + 1) as u16, v);
    }
    Some(Levels { tx, rx })
}

/// One-shot helper: join the metering multicast group and wait up to
/// `timeout` for a datagram from `device_ip`. Returns parsed levels.
///
/// **Caveat:** this is a *passive* listener — it does not ask the
/// device to start metering. For that you need to send the CMC
/// `start_metering` packet (port 8800) which requires host-MAC
/// discovery; see `cmc::host_mac` (TODO). Real Dante Controller will
/// happily satisfy the request from a different process — if it's
/// running on the network and configured to meter the same device,
/// this listener picks up its frames thanks to `SO_REUSEPORT`.
pub async fn capture_one(
    device_ip: IpAddr,
    tx_count: u16,
    rx_count: u16,
    timeout: Duration,
    metering_port: Option<u16>,
) -> Result<Levels> {
    let port = metering_port.unwrap_or(DEFAULT_METERING_PORT);
    let listener =
        MulticastListener::join(MULTICAST_GROUP_CONTROL_MONITORING, port).map_err(Error::Io)?;
    let mut buf = vec![0u8; 4096];
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = match deadline.checked_duration_since(tokio::time::Instant::now()) {
            Some(r) if !r.is_zero() => r,
            _ => return Err(Error::Timeout),
        };
        let res = tokio::time::timeout(remaining, listener.recv_from(&mut buf)).await;
        let (n, addr) = match res {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => return Err(Error::Io(e)),
            Err(_) => return Err(Error::Timeout),
        };
        if addr.ip() != device_ip {
            trace!(src = %addr.ip(), "metering datagram from other source, skipping");
            continue;
        }
        if let Some(levels) = parse_metering_levels(&buf[..n], tx_count, rx_count) {
            return Ok(levels);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_long_form_with_marker() {
        let mut data = vec![0u8; 27 + 4 + 2];
        data[25] = 4;
        data[26] = 2;
        data[27..31].copy_from_slice(&[10, 20, 30, 40]);
        data[31..33].copy_from_slice(&[100, 200]);
        let levels = parse_metering_levels(&data, 4, 2).unwrap();
        assert_eq!(levels.tx[&1], 10);
        assert_eq!(levels.tx[&4], 40);
        assert_eq!(levels.rx[&1], 100);
        assert_eq!(levels.rx[&2], 200);
    }

    #[test]
    fn parses_tail_form_without_marker() {
        // 50-byte datagram with no recognizable marker; tail 6 bytes are
        // 4 tx + 2 rx levels.
        let mut data = vec![0u8; 50];
        let tail = &mut data[44..];
        tail.copy_from_slice(&[1, 2, 3, 4, 50, 60]);
        let levels = parse_metering_levels(&data, 4, 2).unwrap();
        assert_eq!(levels.tx[&1], 1);
        assert_eq!(levels.tx[&4], 4);
        assert_eq!(levels.rx[&1], 50);
        assert_eq!(levels.rx[&2], 60);
    }

    #[test]
    fn rejects_truncated_datagram() {
        let data = vec![0u8; 3];
        assert!(parse_metering_levels(&data, 4, 2).is_none());
    }

    #[test]
    fn empty_counts_returns_empty() {
        let levels = parse_metering_levels(&[1, 2, 3], 0, 0).unwrap();
        assert!(levels.tx.is_empty());
        assert!(levels.rx.is_empty());
    }
}
