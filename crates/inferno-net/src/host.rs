//! Controller-side host discovery: figure out our own IP and MAC for
//! the interface that routes to a Dante device. Both are required by
//! several control packets: CMC metering subscribe, conmon probes
//! (make/model, dante-model, bluetooth, AES67), and the device lock
//! handshake all want the controller's MAC echoed back so the device
//! knows which return path to use.
//!
//! Mirrors `_get_host_mac` and `_get_local_ip` in netaudio
//! `services/cmc.py` + `metering.py`, but uses the cross-platform
//! `network-interface` crate instead of platform-specific ioctls.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

use network_interface::{NetworkInterface, NetworkInterfaceConfig};
use tracing::debug;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy)]
pub struct HostIdentity {
    pub ipv4: Ipv4Addr,
    pub mac: [u8; 6],
}

/// Discover the controller's IPv4 + MAC for the interface that routes
/// to `device_ip`. Uses an unconnected UDP "bind to find local IP"
/// trick (no packet is actually sent), then matches the discovered IP
/// against the host's network interfaces.
pub fn discover_for(device_ip: IpAddr) -> Result<HostIdentity> {
    let local_ip = find_local_ipv4(device_ip)?;
    let interfaces =
        NetworkInterface::show().map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;
    for iface in &interfaces {
        let Some(mac_str) = iface.mac_addr.as_deref() else {
            continue;
        };
        let has_match = iface.addr.iter().any(|a| match a.ip() {
            IpAddr::V4(v4) => v4 == local_ip,
            IpAddr::V6(_) => false,
        });
        if !has_match {
            continue;
        }
        if let Some(mac) = parse_mac(mac_str) {
            debug!(name = %iface.name, ip = %local_ip, "host identity resolved");
            return Ok(HostIdentity {
                ipv4: local_ip,
                mac,
            });
        }
    }
    Err(Error::Protocol(format!(
        "no interface found with IPv4 {local_ip}"
    )))
}

/// Like `discover_for(device_ip)` but uses the multicast metering
/// group as the route target. Suitable when you don't have a specific
/// device IP yet (e.g. early startup).
pub fn discover_default() -> Result<HostIdentity> {
    discover_for(IpAddr::V4(Ipv4Addr::new(224, 0, 0, 231)))
}

fn find_local_ipv4(target: IpAddr) -> Result<Ipv4Addr> {
    let sock = UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], 0)))?;
    // `connect` on a UDP socket only sets a default destination; no
    // packet is sent. The kernel picks a source address based on the
    // routing table — that's exactly the "which interface would talk
    // to this peer" answer we want.
    sock.connect(SocketAddr::new(target, 1))?;
    match sock.local_addr()?.ip() {
        IpAddr::V4(v4) => Ok(v4),
        IpAddr::V6(_) => Err(Error::Protocol(
            "controller has no IPv4 route to target".into(),
        )),
    }
}

fn parse_mac(s: &str) -> Option<[u8; 6]> {
    let mut out = [0u8; 6];
    let parts: Vec<&str> = s.split([':', '-']).collect();
    if parts.len() != 6 {
        return None;
    }
    for (i, p) in parts.iter().enumerate() {
        out[i] = u8::from_str_radix(p, 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_colon_mac() {
        assert_eq!(
            parse_mac("aa:bb:cc:11:22:33"),
            Some([0xaa, 0xbb, 0xcc, 0x11, 0x22, 0x33])
        );
    }
    #[test]
    fn parses_dash_mac() {
        assert_eq!(
            parse_mac("AA-BB-CC-DD-EE-FF"),
            Some([0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff])
        );
    }
    #[test]
    fn rejects_short_mac() {
        assert!(parse_mac("aa:bb:cc").is_none());
    }
}
