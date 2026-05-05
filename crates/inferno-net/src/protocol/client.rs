//! Async UDP client for the Dante ARC port.

use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use rand::RngCore;
use tokio::net::UdpSocket;
use tokio::time::timeout;
use tracing::{debug, instrument, trace, warn};

use super::commands::{self, ChannelDirection, GainDirection};
use super::consts::*;
use super::packet::{DantePacket, DanteResponse};
use super::parser;
use crate::error::{Error, Result};
use crate::host::HostIdentity;
use crate::mcast::{MulticastListener, DEVICE_INFO_PORT, MULTICAST_GROUP_CONTROL_MONITORING};

pub struct DanteClient {
    addr: SocketAddr,
    timeout: Duration,
}

impl DanteClient {
    pub fn new(addr: SocketAddr, timeout: Duration) -> Self {
        Self { addr, timeout }
    }

    /// Bind an ephemeral UDP socket and send a single ARC request, awaiting
    /// one response. The Python reference creates a fresh socket per request;
    /// we mirror that — Dante devices answer to the source port the request
    /// came from.
    #[instrument(level = "debug", skip(self, payload), fields(addr = %self.addr, opcode = format_args!("{:#06x}", opcode), payload_len = payload.len()))]
    async fn query_raw(&self, opcode: u16, payload: &[u8]) -> Result<DanteResponse> {
        let sock = UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], 0))).await?;
        sock.connect(self.addr).await?;
        let txn = (rand::rng().next_u32() & 0xFFFF) as u16;
        let req = DantePacket::build_request(opcode, payload, txn);
        let started = Instant::now();
        trace!(
            txn = format_args!("{:#06x}", txn),
            bytes = req.len(),
            "arc request"
        );
        sock.send(&req).await?;

        let mut buf = vec![0u8; 4096];
        let n = match timeout(self.timeout, sock.recv(&mut buf)).await {
            Ok(r) => r?,
            Err(_) => {
                warn!(
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "arc request timeout"
                );
                return Err(Error::Timeout);
            }
        };
        let resp = DantePacket::parse_response(&buf[..n])?;
        debug!(
            elapsed_ms = started.elapsed().as_millis() as u64,
            result_code = format_args!("{:#06x}", resp.result_code),
            body_len = resp.body.len(),
            "arc response"
        );
        Ok(resp)
    }

    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn channel_count(&self) -> Result<parser::ChannelCount> {
        let resp = self.query_raw(OPCODE_CHANNEL_COUNT, &[0, 0]).await?;
        if resp.result_code != RESULT_CODE_SUCCESS {
            return Err(Error::Protocol(format!(
                "channel_count result_code={:#06x}",
                resp.result_code
            )));
        }
        parser::parse_channel_count(&resp)
    }

    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn device_name(&self) -> Result<String> {
        let resp = self.query_raw(OPCODE_DEVICE_NAME, &[0, 0]).await?;
        if resp.result_code != RESULT_CODE_SUCCESS {
            return Err(Error::Protocol(format!(
                "device_name result_code={:#06x}",
                resp.result_code
            )));
        }
        let trimmed = resp.body.split(|&b| b == 0).next().unwrap_or(&resp.body);
        Ok(String::from_utf8_lossy(trimmed).into_owned())
    }

    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn device_info(&self) -> Result<parser::DeviceInfo> {
        let resp = self.query_raw(OPCODE_DEVICE_INFO, &[0, 0]).await?;
        if resp.result_code != RESULT_CODE_SUCCESS {
            return Err(Error::Protocol(format!(
                "device_info result_code={:#06x}",
                resp.result_code
            )));
        }
        parser::parse_device_info(&resp)
    }

    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn device_settings(&self) -> Result<parser::DeviceSettings> {
        let resp = self.query_raw(OPCODE_DEVICE_SETTINGS, &[0, 0]).await?;
        if resp.result_code != RESULT_CODE_SUCCESS {
            return Err(Error::Protocol(format!(
                "device_settings result_code={:#06x}",
                resp.result_code
            )));
        }
        parser::parse_device_settings(&resp)
    }

    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn rx_channels_page(&self, page: u16) -> Result<Vec<parser::RxChannel>> {
        let starting = page * 16 + 1;
        let mut payload = Vec::with_capacity(8);
        payload.extend_from_slice(&0u16.to_be_bytes());
        payload.extend_from_slice(&1u16.to_be_bytes());
        payload.extend_from_slice(&starting.to_be_bytes());
        payload.extend_from_slice(&0u16.to_be_bytes());
        let resp = self.query_raw(OPCODE_RX_CHANNELS, &payload).await?;
        if resp.result_code != RESULT_CODE_SUCCESS
            && resp.result_code != RESULT_CODE_SUCCESS_EXTENDED
        {
            return Err(Error::Protocol(format!(
                "rx_channels result_code={:#06x}",
                resp.result_code
            )));
        }
        Ok(parser::parse_rx_channels(&resp))
    }

    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn tx_channels_page(&self, page: u16) -> Result<Vec<parser::TxChannel>> {
        let starting = page * 16 + 1;
        let mut payload = Vec::with_capacity(8);
        payload.extend_from_slice(&0u16.to_be_bytes());
        payload.extend_from_slice(&1u16.to_be_bytes());
        payload.extend_from_slice(&starting.to_be_bytes());
        payload.extend_from_slice(&0u16.to_be_bytes());
        let resp = self.query_raw(OPCODE_TX_CHANNEL_INFO, &payload).await?;
        if resp.result_code != RESULT_CODE_SUCCESS
            && resp.result_code != RESULT_CODE_SUCCESS_EXTENDED
        {
            return Err(Error::Protocol(format!(
                "tx_channels result_code={:#06x}",
                resp.result_code
            )));
        }
        Ok(parser::parse_tx_channels(&resp))
    }

    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn all_rx_channels(&self) -> Result<Vec<parser::RxChannel>> {
        let count = self.channel_count().await?;
        let pages = count.rx_count.div_ceil(16);
        let mut out = Vec::new();
        for page in 0..pages {
            let chans = self.rx_channels_page(page).await?;
            let n = chans.len();
            out.extend(chans);
            if n < 16 {
                break;
            }
        }
        Ok(out)
    }

    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn all_tx_channels(&self) -> Result<Vec<parser::TxChannel>> {
        let count = self.channel_count().await?;
        let pages = count.tx_count.div_ceil(16).max(1);
        let mut out = Vec::new();
        for page in (0..pages).step_by(2) {
            out.extend(self.tx_channels_page(page).await?);
        }
        Ok(out)
    }

    // ---- flow-control + lock probe (read-side) ----------------------------

    /// Detect which flow-protocol id the device speaks. Tries each of
    /// `FLOW_PROTOCOL_IDS` in turn with a 2-byte query payload; the first
    /// one returning SUCCESS wins. Returns `None` if the device rejects
    /// all variants (typical for AVIO endpoints with no flow capability).
    /// Mirrors netaudio `detect_flow_protocol`.
    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn detect_flow_protocol(&self) -> Result<Option<u16>> {
        for &protocol_id in FLOW_PROTOCOL_IDS {
            let opcode = if protocol_id == 0x2809 {
                OPCODE_QUERY_TX_FLOWS_2809
            } else {
                OPCODE_QUERY_TX_FLOWS
            };
            let txn = Self::next_txn();
            let pkt = DantePacket::build_flow_request(protocol_id, opcode, &[0, 0], txn);
            match self.send_arc_raw_no_check(&pkt).await {
                Ok(resp) if resp.result_code == RESULT_CODE_SUCCESS => {
                    return Ok(Some(protocol_id))
                }
                _ => continue,
            }
        }
        Ok(None)
    }

    /// Query active TX flows. If `flow_protocol_id` is `None`, runs
    /// `detect_flow_protocol` first.
    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn query_tx_flows(
        &self,
        flow_protocol_id: Option<u16>,
    ) -> Result<(u16, Vec<parser::TxFlow>)> {
        let protocol_id = match flow_protocol_id {
            Some(p) => p,
            None => self.detect_flow_protocol().await?.ok_or_else(|| {
                Error::Protocol("device does not speak any known flow protocol".into())
            })?,
        };
        let opcode = if protocol_id == 0x2809 {
            OPCODE_QUERY_TX_FLOWS_2809
        } else {
            OPCODE_QUERY_TX_FLOWS
        };
        let txn = Self::next_txn();
        let pkt = DantePacket::build_flow_request(protocol_id, opcode, &[0, 0], txn);
        let resp = self.send_arc_raw(&pkt).await?;
        Ok((protocol_id, parser::parse_tx_flows(&resp)))
    }

    /// Probe whether the device is administratively locked. Sends a
    /// privileged write (`set_latency` to 1 s, matching netaudio's
    /// `probe_lock_state`) and inspects `result_code`:
    /// - `RESULT_CODE_LOCK_REJECTION` (0x0600) → locked
    /// - `RESULT_CODE_SUCCESS`        (0x0001) → unlocked
    /// - anything else                          → unknown.
    ///
    /// Note: not strictly read-only — on an unlocked device it
    /// re-applies the 1 s latency value (typically a no-op default).
    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn lock_state(&self) -> Result<Option<bool>> {
        let pkt = commands::cmd_set_latency_us(1_000_000, Self::next_txn());
        let resp = self.send_arc_raw_no_check(&pkt).await?;
        match resp.result_code {
            RESULT_CODE_LOCK_REJECTION => Ok(Some(true)),
            RESULT_CODE_SUCCESS | RESULT_CODE_SUCCESS_EXTENDED => Ok(Some(false)),
            _ => Ok(None),
        }
    }

    /// Send a settings-port (UDP/8700) packet to the device and wait
    /// for the corresponding conmon multicast response on
    /// `224.0.0.231:DEVICE_INFO_PORT (8702)`. Filters by source IP and
    /// expected conmon opcode. Used for make/model, dante-model, and
    /// AES67 probes.
    async fn settings_probe_with_response(
        &self,
        packet: &[u8],
        expected_opcode: u16,
        timeout_dur: Duration,
    ) -> Result<Vec<u8>> {
        let listener =
            MulticastListener::join(MULTICAST_GROUP_CONTROL_MONITORING, DEVICE_INFO_PORT)
                .map_err(Error::Io)?;
        // Send via the same fire-and-forget path the existing settings
        // commands use.
        let sock = UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], 0))).await?;
        let target = SocketAddr::new(self.addr.ip(), DEVICE_SETTINGS_PORT);
        sock.send_to(packet, target).await?;
        debug!("settings probe sent, awaiting multicast response");

        let device_ip = self.addr.ip();
        let deadline = tokio::time::Instant::now() + timeout_dur;
        let mut buf = vec![0u8; 4096];
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
                continue;
            }
            let data = &buf[..n];
            if parser::conmon_opcode(data) == Some(expected_opcode) {
                return Ok(data.to_vec());
            }
        }
    }

    /// Probe make/model + manufacturer + product version. Sends the
    /// conmon `0x00C1` request and parses the `0x00C0` multicast
    /// response. `device_mac` is the *device's* MAC (typically the
    /// lower 6 bytes of its mDNS-advertised EUI-64); pass `[0; 6]` if
    /// unknown — most devices accept the broadcast form.
    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn probe_make_model(&self, device_mac: [u8; 6]) -> Result<parser::MakeModel> {
        let pkt = commands::cmd_make_model_probe(device_mac);
        let resp = self
            .settings_probe_with_response(&pkt, 0x00C0, self.timeout)
            .await?;
        Ok(parser::parse_make_model(&resp))
    }

    /// Probe bluetooth status. Conmon `0x100D` → `0x100E`.
    /// `host_mac` is the *controller's* MAC (echoed back for
    /// correlation) — get it from
    /// [`crate::host::discover_for(device_ip)`].
    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn probe_bluetooth(&self, host_mac: [u8; 6]) -> Result<parser::BluetoothStatus> {
        let pkt = commands::cmd_bluetooth_status_probe(host_mac);
        let resp = self
            .settings_probe_with_response(&pkt, 0x100E, self.timeout)
            .await?;
        parser::parse_bluetooth_status(&resp)
            .ok_or_else(|| Error::Protocol("could not parse bluetooth response".into()))
    }

    /// Probe Dante board codename + name. Conmon `0x0061` →
    /// `0x0060`. Same `device_mac` semantics as `probe_make_model`.
    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn probe_dante_model(&self, device_mac: [u8; 6]) -> Result<parser::DanteModel> {
        let pkt = commands::cmd_dante_model_probe(device_mac);
        let resp = self
            .settings_probe_with_response(&pkt, 0x0060, self.timeout)
            .await?;
        Ok(parser::parse_dante_model(&resp))
    }

    async fn send_arc_raw_no_check(&self, packet: &[u8]) -> Result<DanteResponse> {
        let sock = UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], 0))).await?;
        sock.connect(self.addr).await?;
        sock.send(packet).await?;
        let mut buf = vec![0u8; 4096];
        let n = match timeout(self.timeout, sock.recv(&mut buf)).await {
            Ok(r) => r?,
            Err(_) => return Err(Error::Timeout),
        };
        DantePacket::parse_response(&buf[..n])
    }

    // ---- write path: ARC commands -----------------------------------------

    fn next_txn() -> u16 {
        (rand::rng().next_u32() & 0xFFFF) as u16
    }

    /// Send a pre-built ARC packet, await + validate the response.
    #[instrument(level = "debug", skip(self, packet), fields(addr = %self.addr, packet_len = packet.len()))]
    async fn send_arc_raw(&self, packet: &[u8]) -> Result<DanteResponse> {
        let sock = UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], 0))).await?;
        sock.connect(self.addr).await?;
        let started = Instant::now();
        sock.send(packet).await?;
        let mut buf = vec![0u8; 4096];
        let n = match timeout(self.timeout, sock.recv(&mut buf)).await {
            Ok(r) => r?,
            Err(_) => {
                warn!(
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "arc command timeout"
                );
                return Err(Error::Timeout);
            }
        };
        let resp = DantePacket::parse_response(&buf[..n])?;
        debug!(
            elapsed_ms = started.elapsed().as_millis() as u64,
            result_code = format_args!("{:#06x}", resp.result_code),
            "arc command response"
        );
        if resp.result_code != RESULT_CODE_SUCCESS
            && resp.result_code != RESULT_CODE_SUCCESS_EXTENDED
        {
            return Err(Error::Protocol(format!(
                "command failed: result_code={:#06x}",
                resp.result_code
            )));
        }
        Ok(resp)
    }

    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn set_device_name(&self, name: &str) -> Result<()> {
        let pkt = commands::cmd_set_device_name(name, Self::next_txn());
        self.send_arc_raw(&pkt).await?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn reset_device_name(&self) -> Result<()> {
        let pkt = commands::cmd_reset_device_name(Self::next_txn());
        self.send_arc_raw(&pkt).await?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn set_channel_name(
        &self,
        direction: ChannelDirection,
        channel_number: u8,
        new_name: &str,
    ) -> Result<()> {
        let pkt =
            commands::cmd_set_channel_name(direction, channel_number, new_name, Self::next_txn());
        self.send_arc_raw(&pkt).await?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn reset_channel_name(
        &self,
        direction: ChannelDirection,
        channel_number: u8,
    ) -> Result<()> {
        let pkt = commands::cmd_reset_channel_name(direction, channel_number, Self::next_txn());
        self.send_arc_raw(&pkt).await?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn add_subscription(
        &self,
        rx_channel: u16,
        tx_channel_name: &str,
        tx_device_name: &str,
    ) -> Result<()> {
        let pkt = commands::cmd_add_subscriptions(
            &[(rx_channel, tx_channel_name, tx_device_name)],
            Self::next_txn(),
        )?;
        self.send_arc_raw(&pkt).await?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self, subs), fields(addr = %self.addr, count = subs.len()))]
    pub async fn add_subscriptions_bulk(&self, subs: &[(u16, &str, &str)]) -> Result<()> {
        let pkt = commands::cmd_add_subscriptions(subs, Self::next_txn())?;
        self.send_arc_raw(&pkt).await?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn remove_subscription(&self, rx_channel: u32) -> Result<()> {
        let pkt = commands::cmd_remove_subscriptions(&[rx_channel], Self::next_txn())?;
        self.send_arc_raw(&pkt).await?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self, rx_channels), fields(addr = %self.addr, count = rx_channels.len()))]
    pub async fn remove_subscriptions_bulk(&self, rx_channels: &[u32]) -> Result<()> {
        let pkt = commands::cmd_remove_subscriptions(rx_channels, Self::next_txn())?;
        self.send_arc_raw(&pkt).await?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn set_latency_us(&self, latency_us: u32) -> Result<()> {
        let pkt = commands::cmd_set_latency_us(latency_us, Self::next_txn());
        self.send_arc_raw(&pkt).await?;
        Ok(())
    }

    // ---- write path: settings-port commands (UDP/8700) --------------------

    /// Fire-and-forget Settings-port send. These commands typically don't
    /// produce a synchronous reply on the same socket; callers re-poll device
    /// state to verify.
    #[instrument(level = "debug", skip(self, packet), fields(addr = %self.addr, packet_len = packet.len()))]
    async fn send_settings_raw(&self, packet: &[u8]) -> Result<()> {
        let sock = UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], 0))).await?;
        let target = SocketAddr::new(self.addr.ip(), DEVICE_SETTINGS_PORT);
        sock.send_to(packet, target).await?;
        debug!("settings packet sent (fire-and-forget)");
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn reboot(&self, host_mac: Option<[u8; 6]>) -> Result<()> {
        self.send_settings_raw(&commands::cmd_reboot(host_mac))
            .await
    }

    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn identify(&self) -> Result<()> {
        self.send_settings_raw(&commands::cmd_identify()).await
    }

    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn set_sample_rate(&self, sample_rate: u32) -> Result<()> {
        self.send_settings_raw(&commands::cmd_set_sample_rate(sample_rate))
            .await
    }

    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn set_encoding(&self, encoding: u8) -> Result<()> {
        self.send_settings_raw(&commands::cmd_set_encoding(encoding))
            .await
    }

    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn set_gain_level(
        &self,
        direction: GainDirection,
        channel_number: u8,
        gain_level: u8,
    ) -> Result<()> {
        self.send_settings_raw(&commands::cmd_set_gain_level(
            direction,
            channel_number,
            gain_level,
        ))
        .await
    }

    /// Subscribe the controller to the device's metering multicast
    /// stream. Sends a CMC port (8800) `command_metering_start` packet;
    /// caller is responsible for listening on
    /// `224.0.0.231:metering_port` (see
    /// [`crate::metering::capture_one`] or
    /// [`crate::mcast::MulticastListener`]).
    ///
    /// `host` should come from
    /// [`crate::host::discover_for(device_ip)`]. `device_name` must
    /// match what the device reports via mDNS / `device_name`.
    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn start_metering(
        &self,
        device_name: &str,
        host: HostIdentity,
        metering_port: u16,
    ) -> Result<()> {
        let pkt = commands::cmd_metering_start(
            device_name,
            host.ipv4.octets(),
            host.mac,
            metering_port,
            true,
            Self::next_txn(),
        );
        let target = SocketAddr::new(self.addr.ip(), DEVICE_CONTROL_PORT);
        let sock = UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], 0))).await?;
        sock.send_to(&pkt, target).await?;
        Ok(())
    }

    /// Tell the device to stop its metering stream.
    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn stop_metering(&self, device_name: &str, host: HostIdentity) -> Result<()> {
        let pkt = commands::cmd_metering_stop(device_name, host.mac, Self::next_txn());
        let target = SocketAddr::new(self.addr.ip(), DEVICE_CONTROL_PORT);
        let sock = UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], 0))).await?;
        sock.send_to(&pkt, target).await?;
        Ok(())
    }

    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn enable_aes67(&self, enabled: bool, host_mac: Option<[u8; 6]>) -> Result<()> {
        self.send_settings_raw(&commands::cmd_enable_aes67(enabled, host_mac))
            .await
    }

    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn set_preferred_leader(
        &self,
        is_preferred: bool,
        clock_source: u16,
        host_mac: Option<[u8; 6]>,
    ) -> Result<()> {
        self.send_settings_raw(&commands::cmd_set_preferred_leader(
            is_preferred,
            clock_source,
            host_mac,
        ))
        .await
    }

    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn set_interface_dhcp(&self, host_mac: Option<[u8; 6]>) -> Result<()> {
        self.send_settings_raw(&commands::cmd_set_interface_dhcp(host_mac))
            .await
    }

    #[instrument(level = "debug", skip(self), fields(addr = %self.addr))]
    pub async fn set_interface_static(
        &self,
        ip: IpAddr,
        netmask: IpAddr,
        dns: IpAddr,
        gateway: IpAddr,
        host_mac: Option<[u8; 6]>,
    ) -> Result<()> {
        let to_v4 = |a: IpAddr| -> Result<[u8; 4]> {
            match a {
                IpAddr::V4(v) => Ok(v.octets()),
                IpAddr::V6(_) => Err(Error::Protocol(
                    "ipv6 not supported for static config".into(),
                )),
            }
        };
        let pkt = commands::cmd_set_interface_static(
            to_v4(ip)?,
            to_v4(netmask)?,
            to_v4(dns)?,
            to_v4(gateway)?,
            host_mac,
        );
        self.send_settings_raw(&pkt).await
    }
}
