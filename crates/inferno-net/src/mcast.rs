//! IPv4 multicast listener primitive.
//!
//! Dante exposes several control + monitoring streams as IPv4 multicast:
//!
//! | Group           | Port | Used by                        |
//! |-----------------|------|---------------------------------|
//! | `224.0.0.231`   | 8702 | conmon device-info / probe responses (#9) |
//! | `224.0.0.231`   | 8751 | metering (Dante Controller default)       |
//! | `224.0.0.231`   | 8752 | metering (our daemon default, #6)         |
//! | `224.0.0.233`   | 8708 | heartbeat (#8)                             |
//!
//! `MulticastListener::join` opens a `SO_REUSEADDR + SO_REUSEPORT`
//! datagram socket bound to `0.0.0.0:<port>`, joins the group on the
//! default interface, and exposes a `recv_from` async helper. Caller is
//! responsible for filtering datagrams by source IP and parsing the
//! body (see `crate::metering`).

use std::io;
use std::net::{Ipv4Addr, SocketAddr};

use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;
use tracing::debug;

pub const MULTICAST_GROUP_CONTROL_MONITORING: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 231);
pub const MULTICAST_GROUP_HEARTBEAT: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 233);

/// Default port for `224.0.0.231` metering stream when published by our
/// own daemon. Real Dante Controller uses `8751`; we use `8752` so both
/// can coexist on the same host.
pub const DEFAULT_METERING_PORT: u16 = 8752;
pub const DANTE_CONTROLLER_METERING_PORT: u16 = 8751;
pub const DEVICE_INFO_PORT: u16 = 8702;
pub const HEARTBEAT_PORT: u16 = 8708;

pub struct MulticastListener {
    sock: UdpSocket,
    group: Ipv4Addr,
    port: u16,
}

impl MulticastListener {
    /// Join `group:port` on the default interface (0.0.0.0). The socket
    /// is set to reuse address+port so multiple processes (e.g. Dante
    /// Controller and us) can listen simultaneously.
    pub fn join(group: Ipv4Addr, port: u16) -> io::Result<Self> {
        let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        sock.set_reuse_address(true)?;
        #[cfg(unix)]
        sock.set_reuse_port(true)?;
        sock.set_nonblocking(true)?;
        let bind: SocketAddr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, port));
        sock.bind(&bind.into())?;
        sock.join_multicast_v4(&group, &Ipv4Addr::UNSPECIFIED)?;
        let std_sock: std::net::UdpSocket = sock.into();
        let sock = UdpSocket::from_std(std_sock)?;
        debug!(%group, port, "joined multicast group");
        Ok(Self { sock, group, port })
    }

    pub fn group(&self) -> Ipv4Addr {
        self.group
    }
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Receive one datagram. Returns the source address along with the
    /// bytes; caller decides whether to keep, filter, or parse.
    pub async fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        self.sock.recv_from(buf).await
    }
}
