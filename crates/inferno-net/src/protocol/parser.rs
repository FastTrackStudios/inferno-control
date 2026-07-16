//! Response body parsers for ARC opcodes.
//!
//! Pointer offsets in Dante responses are relative to the *full packet*; the
//! response header is 10 bytes, so we subtract 10 to index into the body.

use serde::{Deserialize, Serialize};

use super::consts::*;
use super::packet::{read_cstr, DanteResponse};
use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ChannelCount {
    pub capability_flags: u16,
    pub tx_count: u16,
    pub rx_count: u16,
    pub tx_active: u16,
    pub rx_active: u16,
    pub max_tx_flows: u16,
    pub max_rx_flows: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub model_name: String,
    pub display_name: String,
    pub model_code: String,
    pub port: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeviceSettings {
    pub sample_rate: Option<u32>,
    pub latency_us: Option<u32>,
    pub min_latency_us: Option<u32>,
    pub max_latency_us: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RxChannel {
    pub number: u16,
    pub name: String,
    pub tx_channel_name: Option<String>,
    pub tx_device_name: Option<String>,
    pub status_code: u16,
    pub subscription_status: u16,
    pub sample_rate: Option<u32>,
}

impl RxChannel {
    /// Whether this channel carries a working subscription. Status
    /// codes observed in the wild: hardware Dante reports 9 (dynamic /
    /// unicast), 10 (static / multicast) or 14 (manual) for flowing
    /// audio; inferno-aoip reports 1 for its connected subscriptions
    /// (the Dante spec calls 1 "unresolved" — treat it as active since
    /// that's what a healthy Inferno device emits).
    pub fn subscription_is_active(&self) -> bool {
        (self.tx_channel_name.is_some() || self.tx_device_name.is_some())
            && matches!(self.subscription_status, 1 | 9 | 10 | 14)
    }
}

/// Human label for an ARC subscription-status code (best-effort — only
/// codes observed on real devices are named).
pub fn subscription_status_label(code: u16) -> &'static str {
    match code {
        0 => "none",
        1 => "connected (inferno) / unresolved",
        9 => "dynamic",
        10 => "static",
        14 => "manual",
        _ => "unknown",
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxChannel {
    pub number: u16,
    pub name: String,
    pub sample_rate: Option<u32>,
    pub group: u16,
}

fn be_u16(buf: &[u8], i: usize) -> u16 {
    u16::from_be_bytes([buf[i], buf[i + 1]])
}
fn be_u32(buf: &[u8], i: usize) -> u32 {
    u32::from_be_bytes([buf[i], buf[i + 1], buf[i + 2], buf[i + 3]])
}

/// Some hardware puts a non-pointer word in the sample-rate slot; the
/// dereferenced garbage (e.g. 117509122) is filtered by plausibility.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn plausible_rate(v: &u32) -> bool {
    (8_000..=768_000).contains(v)
}

fn string_at_pointer(body: &[u8], pointer: u16) -> String {
    if pointer == 0 {
        return String::new();
    }
    let p = pointer as usize;
    if p < 10 {
        return String::new();
    }
    let off = p - 10;
    if off >= body.len() {
        return String::new();
    }
    read_cstr(body, off)
}

fn u32_at_pointer(body: &[u8], pointer: u16) -> Option<u32> {
    if pointer == 0 {
        return None;
    }
    let p = pointer as usize;
    if p < 10 {
        return None;
    }
    let off = p - 10;
    if off + 4 > body.len() {
        return None;
    }
    Some(be_u32(body, off))
}

pub fn parse_channel_count(resp: &DanteResponse) -> Result<ChannelCount> {
    let b = &resp.body;
    if b.len() < 14 {
        return Err(Error::Protocol(format!(
            "channel count body too short: {}",
            b.len()
        )));
    }
    Ok(ChannelCount {
        capability_flags: be_u16(b, 0),
        tx_count: be_u16(b, 2),
        rx_count: be_u16(b, 4),
        tx_active: be_u16(b, 6),
        rx_active: be_u16(b, 8),
        max_tx_flows: be_u16(b, 10),
        max_rx_flows: be_u16(b, 12),
    })
}

pub fn parse_device_info(resp: &DanteResponse) -> Result<DeviceInfo> {
    let b = &resp.body;
    if b.len() < 18 {
        return Err(Error::Protocol(format!(
            "device info body too short: {}",
            b.len()
        )));
    }
    let code_ptr = be_u16(b, 6);
    let port_ptr = be_u16(b, 8);
    let model_ptr = be_u16(b, 12);
    let display_ptr = be_u16(b, 14);
    Ok(DeviceInfo {
        model_name: string_at_pointer(b, model_ptr),
        display_name: string_at_pointer(b, display_ptr),
        model_code: string_at_pointer(b, code_ptr),
        port: string_at_pointer(b, port_ptr),
    })
}

pub fn parse_device_settings(resp: &DanteResponse) -> Result<DeviceSettings> {
    let b = &resp.body;
    if b.len() < 2 {
        return Err(Error::Protocol("device settings body too short".into()));
    }
    let record_count = b[1] as usize;
    let mut s = DeviceSettings::default();
    let mut off = 2usize;
    for _ in 0..record_count {
        if off + 4 > b.len() {
            break;
        }
        let info_code = be_u16(b, off);
        let value_ptr = be_u16(b, off + 2);
        off += 4;
        if let Some(v) = u32_at_pointer(b, value_ptr) {
            match info_code {
                DEVICE_SETTINGS_INFO_SAMPLE_RATE => s.sample_rate = Some(v),
                DEVICE_SETTINGS_INFO_LATENCY => s.latency_us = Some(v),
                DEVICE_SETTINGS_INFO_LATENCY_MIN => s.min_latency_us = Some(v),
                DEVICE_SETTINGS_INFO_LATENCY_MAX => s.max_latency_us = Some(v),
                _ => {}
            }
        }
    }
    Ok(s)
}

pub fn parse_rx_channels(resp: &DanteResponse) -> Vec<RxChannel> {
    const RECORD: usize = 20;
    const HEADER: usize = 2;
    let b = &resp.body;
    let mut out = Vec::new();
    if b.len() < HEADER + RECORD {
        return out;
    }
    let mut start = HEADER;
    while start + RECORD <= b.len() && out.len() < 64 {
        let r = &b[start..start + RECORD];
        let channel_number = be_u16(r, 0);
        let _flags = be_u16(r, 2);
        let sample_rate_ptr = be_u16(r, 4);
        let tx_channel_ptr = be_u16(r, 6);
        let tx_device_ptr = be_u16(r, 8);
        let rx_channel_ptr = be_u16(r, 10);
        let status = be_u16(r, 12);
        let sub_status = be_u16(r, 14);
        // Records carry consecutive channel numbers and the string pool
        // sits right behind them. Inferno zero-pads (the `== 0` break);
        // hardware (Galaxy32, Apollo) does NOT — without the sequence
        // guard the walk continues into the string pool and fabricates
        // channels out of name bytes (a 64-ch console "grew" 189 RX).
        let expected = out.last().map(|c: &RxChannel| c.number + 1);
        if channel_number == 0 || expected.is_some_and(|e| channel_number != e) {
            break;
        }
        let name = string_at_pointer(b, rx_channel_ptr);
        let tx_channel_name = {
            let s = string_at_pointer(b, tx_channel_ptr);
            (!s.is_empty()).then_some(s)
        };
        let tx_device_name = {
            let s = string_at_pointer(b, tx_device_ptr);
            (!s.is_empty()).then_some(s)
        };
        let sample_rate = u32_at_pointer(b, sample_rate_ptr).filter(plausible_rate);
        out.push(RxChannel {
            number: channel_number,
            name,
            tx_channel_name,
            tx_device_name,
            status_code: status,
            subscription_status: sub_status,
            sample_rate,
        });
        start += RECORD;
    }
    out
}

pub fn parse_tx_channels(resp: &DanteResponse) -> Vec<TxChannel> {
    const RECORD: usize = 8;
    const HEADER: usize = 2;
    let b = &resp.body;
    let mut out: Vec<TxChannel> = Vec::new();
    if b.len() < HEADER + RECORD {
        return out;
    }
    let mut start = HEADER;
    while start + RECORD <= b.len() && out.len() < 128 {
        let r = &b[start..start + RECORD];
        let channel_number = be_u16(r, 0);
        let sample_rate_ptr = be_u16(r, 2);
        let group = be_u16(r, 4);
        let name_ptr = be_u16(r, 6);
        let expected = out.last().map(|c| c.number + 1).unwrap_or(channel_number);
        if channel_number == 0 || channel_number != expected {
            break;
        }
        let sample_rate = u32_at_pointer(b, sample_rate_ptr).filter(plausible_rate);
        out.push(TxChannel {
            number: channel_number,
            name: string_at_pointer(b, name_ptr),
            sample_rate,
            group,
        });
        start += RECORD;
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxFlow {
    pub flow_number: u16,
    /// Raw flow-type word; `FLOW_TYPE_MULTICAST` (0x0002) is the common case.
    pub flow_type: u16,
    pub sample_rate: u32,
    pub encoding: u16,
    /// Frames per packet.
    pub fpp: u16,
    /// TX channel numbers carried by this flow (multicast flows only).
    pub channels: Vec<u16>,
}

impl TxFlow {
    pub fn is_multicast(&self) -> bool {
        self.flow_type == FLOW_TYPE_MULTICAST
    }
}

/// Parse a TX-flow query response. The response header is 10 bytes; the
/// body starts with `max_flow_slots u8, active_count u8` followed by
/// `active_count` records of `(flow_number u16, record_pointer u16)`.
/// Each record_pointer is an offset into the *full packet* (subtract 10
/// for the body offset), pointing at a flow descriptor.
///
/// Mirrors `_parse_flow_record` in netaudio's
/// `dante/services/arc.py:291-321`.
pub fn parse_tx_flows(resp: &DanteResponse) -> Vec<TxFlow> {
    let body = &resp.body;
    if body.len() < 2 {
        return Vec::new();
    }
    let active_count = body[1] as usize;
    let mut flows = Vec::with_capacity(active_count);
    let mut offset = 2usize;
    for _ in 0..active_count {
        if offset + 4 > body.len() {
            break;
        }
        let flow_number = be_u16(body, offset);
        let record_pointer = be_u16(body, offset + 2) as usize;
        offset += 4;
        // Pointers are relative to the whole packet; the body starts at
        // packet offset 10.
        let record_offset = match record_pointer.checked_sub(10) {
            Some(v) => v,
            None => continue,
        };
        if record_offset + 12 > body.len() {
            continue;
        }
        let r = &body[record_offset..];
        let flow_type = be_u16(r, 2);
        let sample_rate = be_u32(r, 4);
        let encoding = be_u16(r, 8);
        let fpp = be_u16(r, 10);
        let mut channels = Vec::new();
        if flow_type == FLOW_TYPE_MULTICAST && r.len() >= 24 {
            let channel_count = be_u16(r, 22) as usize;
            for i in 0..channel_count {
                let off = 24 + i * 2;
                if off + 2 > r.len() {
                    break;
                }
                channels.push(be_u16(r, off));
            }
        }
        flows.push(TxFlow {
            flow_number,
            flow_type,
            sample_rate,
            encoding,
            fpp,
            channels,
        });
    }
    flows
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MakeModel {
    pub manufacturer: String,
    pub product_name: String,
    /// `major.minor.build` formatted version, when reported.
    pub product_version: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DanteModel {
    pub board_codename: String,
    pub board_name: String,
}

/// Conmon offset constants from netaudio
/// `dante/services/notification.py` (`CONMON_*`).
const CONMON_MANUFACTURER_OFFSET: usize = 0x4C;
const CONMON_PRODUCT_NAME_OFFSET: usize = 0xCC;
const CONMON_PRODUCT_VERSION_OFFSET: usize = 0x14C;
const CONMON_BOARD_CODENAME_OFFSET: usize = 0x2C;
const CONMON_BOARD_NAME_OFFSET: usize = 0x58;

/// Read an ASCII C-string of bounded length, stripping leading bytes
/// `< 0x20` (some firmwares prepend a length-byte prefix) and
/// terminating at the first NUL. Returns an empty string on bounds
/// errors. Mirrors `_extract_null_terminated_string` in
/// netaudio's notification parser.
fn read_conmon_string(data: &[u8], start: usize, end: usize) -> String {
    if start >= data.len() {
        return String::new();
    }
    let real_end = end.min(data.len());
    let raw = &data[start..real_end];
    let trimmed = raw.iter().position(|&b| b >= 0x20).unwrap_or(raw.len());
    let region = &raw[trimmed..];
    let nul = region.iter().position(|&b| b == 0).unwrap_or(region.len());
    String::from_utf8_lossy(&region[..nul]).into_owned()
}

/// Parse a make/model conmon multicast response (opcode `0x00C0`).
/// Caller is responsible for collecting the datagram from
/// `224.0.0.231:DEVICE_INFO_PORT (8702)` and identifying the opcode at
/// `magic_pos+10` (where `magic_pos` is `data.find(b"Audinate", 4)`).
pub fn parse_make_model(data: &[u8]) -> MakeModel {
    let manufacturer =
        read_conmon_string(data, CONMON_MANUFACTURER_OFFSET, CONMON_PRODUCT_NAME_OFFSET);
    let product_name = read_conmon_string(
        data,
        CONMON_PRODUCT_NAME_OFFSET,
        CONMON_PRODUCT_VERSION_OFFSET,
    );
    let product_version = if data.len() >= CONMON_PRODUCT_VERSION_OFFSET + 4 {
        let p = CONMON_PRODUCT_VERSION_OFFSET;
        let major = data[p];
        let minor = data[p + 1];
        let build = data[p + 3];
        if (major, minor, build) == (0, 0, 0) {
            None
        } else {
            Some(format!("{major}.{minor}.{build}"))
        }
    } else {
        None
    };
    MakeModel {
        manufacturer,
        product_name,
        product_version,
    }
}

/// Parse a dante-model conmon multicast response (opcode `0x0060`).
pub fn parse_dante_model(data: &[u8]) -> DanteModel {
    DanteModel {
        board_codename: read_conmon_string(
            data,
            CONMON_BOARD_CODENAME_OFFSET,
            CONMON_BOARD_NAME_OFFSET,
        ),
        board_name: read_conmon_string(
            data,
            CONMON_BOARD_NAME_OFFSET,
            CONMON_BOARD_NAME_OFFSET + 64,
        ),
    }
}

/// Locate the conmon opcode in a multicast response. Returns the u16 at
/// offset `magic_pos + 10` where `magic_pos` is the position of the
/// `Audinate` magic string (searched starting at offset 4 to skip the
/// outer mcast header). `None` if the magic isn't present.
pub fn conmon_opcode(data: &[u8]) -> Option<u16> {
    if data.len() < 4 {
        return None;
    }
    // Substring search for b"Audinate" starting at offset 4.
    let needle = b"Audinate";
    let start = 4;
    let pos = (start..=data.len().saturating_sub(needle.len()))
        .find(|&i| &data[i..i + needle.len()] == needle)?;
    if pos + 12 > data.len() {
        return None;
    }
    Some(u16::from_be_bytes([data[pos + 10], data[pos + 11]]))
}

#[cfg(test)]
mod conmon_tests {
    use super::*;

    fn synth_response(opcode: u16, fill_at: &[(usize, &[u8])]) -> Vec<u8> {
        // 32-byte mcast outer header, then "Audinate" magic at offset 32,
        // then opcode at +10 from magic. Fill payload regions for the
        // string offsets we want to test.
        let mut data = vec![0u8; 0x200];
        data[0..2].copy_from_slice(&0xFFFFu16.to_be_bytes()); // start_code
        data[32..40].copy_from_slice(b"Audinate");
        data[42..44].copy_from_slice(&opcode.to_be_bytes());
        for (off, bytes) in fill_at {
            data[*off..*off + bytes.len()].copy_from_slice(bytes);
        }
        data
    }

    #[test]
    fn parses_make_model() {
        let data = synth_response(
            0x00C0,
            &[
                (CONMON_MANUFACTURER_OFFSET, b"Audinate Pty Ltd\0"),
                (CONMON_PRODUCT_NAME_OFFSET, b"AVIO-USB\0"),
                (CONMON_PRODUCT_VERSION_OFFSET, &[1, 2, 0, 7]),
            ],
        );
        assert_eq!(conmon_opcode(&data), Some(0x00C0));
        let mm = parse_make_model(&data);
        assert_eq!(mm.manufacturer, "Audinate Pty Ltd");
        assert_eq!(mm.product_name, "AVIO-USB");
        assert_eq!(mm.product_version.as_deref(), Some("1.2.7"));
    }

    #[test]
    fn parses_dante_model() {
        let data = synth_response(
            0x0060,
            &[
                (CONMON_BOARD_CODENAME_OFFSET, b"BRAVO\0"),
                (CONMON_BOARD_NAME_OFFSET, b"Brooklyn II\0"),
            ],
        );
        let dm = parse_dante_model(&data);
        assert_eq!(dm.board_codename, "BRAVO");
        assert_eq!(dm.board_name, "Brooklyn II");
    }

    #[test]
    fn make_model_omits_zero_version() {
        let data = synth_response(
            0x00C0,
            &[
                (CONMON_MANUFACTURER_OFFSET, b"Vendor\0"),
                (CONMON_PRODUCT_NAME_OFFSET, b"Product\0"),
                (CONMON_PRODUCT_VERSION_OFFSET, &[0, 0, 0, 0]),
            ],
        );
        let mm = parse_make_model(&data);
        assert!(mm.product_version.is_none());
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BluetoothStatus {
    /// Bluetooth radio off / no controller in this product.
    Unsupported,
    /// Radio on but nothing paired.
    Disconnected,
    /// Connected to the named device.
    Connected(String),
}

/// Parse a bluetooth-status conmon response (opcode `0x100E`). The
/// format is *not* the simple fixed-offset conmon layout — it's a
/// nested protobuf-style envelope of `[tag][len][body]` fields. The
/// wrapper:
///
/// ```text
/// [36] 0x12              field 2 (length-delimited)
/// [37] field1_total_len  (varint, but always single-byte in practice)
/// [38] 0x0a              field 1 (length-delimited, the actual payload)
/// [39] field1_len        (single-byte payload length)
/// [40 .. 40+field1_len]  payload
/// ```
///
/// Inside the wrapper:
/// - optional `0x18 <varint>` field 3 (skipped)
/// - mandatory `0x22 <len> <body>` field 4 — the connection-status
///   record, which itself looks like `0x0a <len> ... 0x12 <len> ...`
///   with an inner `0x08 0x01` (connected) / `0x08 0x02` (disconnected)
///   marker. When connected, the marker is followed by `0x12 <len>
///   <utf8_device_name>`.
///
/// Mirrors `parse_bluetooth_status` + `_find_bluetooth_name` in
/// netaudio's `device_parser.py:30-90`.
pub fn parse_bluetooth_status(data: &[u8]) -> Option<BluetoothStatus> {
    if data.len() < 50 || data[36] != 0x12 || data[38] != 0x0a {
        return None;
    }
    let field1_len = data[39] as usize;
    let mut pos = 40 + field1_len;

    // Optional field 3 (varint).
    if pos < data.len() && data[pos] == 0x18 {
        pos += 1;
        while pos < data.len() && data[pos] & 0x80 != 0 {
            pos += 1;
        }
        pos += 1;
    }

    if pos >= data.len() || data[pos] != 0x22 {
        return None;
    }
    pos += 1;
    if pos >= data.len() {
        return None;
    }
    let field4_len = data[pos] as usize;
    pos += 1;
    let field4_end = pos + field4_len;
    if field4_end > data.len() {
        return None;
    }
    let inner = &data[pos..field4_end];

    // Inner record: `0x0a <len> ... 0x12 <len> ...`.
    if inner.len() < 5 || inner[0] != 0x0a || inner[2] != 0x12 {
        return Some(BluetoothStatus::Unsupported);
    }
    // Search for the `08 01 12` connected-marker. `08 02` = disconnected.
    let marker_connected = inner.windows(3).position(|w| w == [0x08, 0x01, 0x12]);
    if let Some(m) = marker_connected {
        let name_len_pos = m + 3;
        if name_len_pos >= inner.len() {
            return Some(BluetoothStatus::Disconnected);
        }
        let name_len = inner[name_len_pos] as usize;
        let name_start = name_len_pos + 1;
        let name_end = name_start + name_len;
        if name_end > inner.len() {
            return Some(BluetoothStatus::Disconnected);
        }
        let name = String::from_utf8_lossy(&inner[name_start..name_end]).into_owned();
        return Some(BluetoothStatus::Connected(name));
    }
    if inner.windows(2).any(|w| w == [0x08, 0x02]) {
        return Some(BluetoothStatus::Disconnected);
    }
    Some(BluetoothStatus::Unsupported)
}

#[cfg(test)]
mod bluetooth_tests {
    use super::*;

    fn wrap(inner: &[u8]) -> Vec<u8> {
        // Synthesize a minimal 50+ byte response that the wrapper parser
        // accepts. The wrapper expects:
        //   [36] = 0x12
        //   [37] = wrapper_len (1 byte)
        //   [38] = 0x0a
        //   [39] = field1_len = 0  (we use the optional `0x18 ...` form
        //                            and the field4 follows immediately)
        let mut data = vec![0u8; 50];
        data[36] = 0x12;
        data[37] = 0x10; // wrapper len (arbitrary)
        data[38] = 0x0a;
        data[39] = 0; // field1_len = 0
                      // pos is now 40. Add field 3 varint (0x18 0x00) then field 4.
        let mut tail = vec![0x18, 0x00, 0x22, inner.len() as u8];
        tail.extend_from_slice(inner);
        // Pad data so wrapper reaches into tail at offset 40.
        data.truncate(40);
        data.extend(tail);
        // Pad to >= 50.
        while data.len() < 50 {
            data.push(0);
        }
        data
    }

    #[test]
    fn parses_connected_with_name() {
        let name = b"My iPhone";
        let mut inner = vec![0x0a, 0x10, 0x12, 0x08]; // outer record header
        inner.extend_from_slice(b"abcdefgh");
        // connected marker + name
        inner.extend_from_slice(&[0x08, 0x01, 0x12, name.len() as u8]);
        inner.extend_from_slice(name);
        let data = wrap(&inner);
        match parse_bluetooth_status(&data) {
            Some(BluetoothStatus::Connected(n)) => assert_eq!(n, "My iPhone"),
            other => panic!("expected Connected, got {other:?}"),
        }
    }

    #[test]
    fn parses_disconnected() {
        let mut inner = vec![0x0a, 0x04, 0x12, 0x02];
        inner.extend_from_slice(&[0xaa, 0xbb]); // padding
        inner.extend_from_slice(&[0x08, 0x02]); // disconnected marker
        let data = wrap(&inner);
        assert_eq!(
            parse_bluetooth_status(&data),
            Some(BluetoothStatus::Disconnected)
        );
    }

    #[test]
    fn rejects_too_short() {
        assert_eq!(parse_bluetooth_status(&[0u8; 30]), None);
    }
}
