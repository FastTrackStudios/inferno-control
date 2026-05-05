//! Builders for write-side ARC and Settings-port commands.
//!
//! Mirrors `netaudio/dante/device_commands.py`. Two header formats are in
//! play here:
//!
//! 1. **ARC** (UDP/4440): `[PROTOCOL_ID (2)] [length (2)] [txn (2)] [opcode (2)] [payload..]`
//! 2. **Settings** (UDP/8700): `[PROTOCOL_SETTINGS (2)] [0x00] [length (1)] [payload..]`
//!    where length = payload + 4. Used for reboot, identify, sample-rate set,
//!    encoding, gain, AES67, interface config, preferred-leader, etc.
//!
//! All Settings-port payloads start with `[seq (2)] [0 (2)] [mac (6)] [0 (2)]
//! [magic ("Audinate" + 0x07 + subop)]` followed by command-specific bytes.

use super::consts::*;
use super::packet::DantePacket;
use crate::error::{Error, Result};

const SETTINGS_MAGIC_BASE: &[u8; 8] = b"Audinate";

#[derive(Debug, Clone, Copy)]
pub enum ChannelDirection {
    Rx,
    Tx,
}

/// ARC: rename the device.
pub fn cmd_set_device_name(name: &str, txn: u16) -> Vec<u8> {
    let mut payload = Vec::with_capacity(2 + name.len() + 1);
    payload.extend_from_slice(&0u16.to_be_bytes());
    payload.extend_from_slice(name.as_bytes());
    payload.push(0);
    DantePacket::build_request(OPCODE_DEVICE_NAME_SET, &payload, txn)
}

/// ARC: reset device name to factory default (empty payload).
pub fn cmd_reset_device_name(txn: u16) -> Vec<u8> {
    DantePacket::build_request(OPCODE_DEVICE_NAME_SET, &[0, 0], txn)
}

/// ARC: rename a TX or RX channel.
pub fn cmd_set_channel_name(
    direction: ChannelDirection,
    channel_number: u8,
    new_name: &str,
    txn: u16,
) -> Vec<u8> {
    let name_bytes = new_name.as_bytes();
    match direction {
        ChannelDirection::Rx => {
            let mut payload = Vec::with_capacity(12 + name_bytes.len() + 1);
            payload.extend_from_slice(&0u16.to_be_bytes()); // pad
            payload.extend_from_slice(&[0x02, 0x01, 0x00, channel_number]);
            payload.extend_from_slice(&0x14u16.to_be_bytes());
            payload.extend_from_slice(&[0, 0, 0, 0]);
            payload.extend_from_slice(name_bytes);
            payload.push(0);
            DantePacket::build_request(OPCODE_RX_CHANNEL_NAME_SET, &payload, txn)
        }
        ChannelDirection::Tx => {
            let mut payload = Vec::with_capacity(14 + name_bytes.len() + 1);
            payload.extend_from_slice(&0u16.to_be_bytes());
            payload.extend_from_slice(&[0x02, 0x01, 0x00, 0x00, 0x00, channel_number]);
            payload.extend_from_slice(&0x18u16.to_be_bytes());
            payload.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
            payload.extend_from_slice(name_bytes);
            payload.push(0);
            DantePacket::build_request(OPCODE_TX_CHANNEL_NAME_SET, &payload, txn)
        }
    }
}

/// ARC: reset a channel's name to factory default.
pub fn cmd_reset_channel_name(
    direction: ChannelDirection,
    channel_number: u8,
    txn: u16,
) -> Vec<u8> {
    cmd_set_channel_name(direction, channel_number, "", txn)
}

/// ARC: add 1..=16 subscriptions in a single packet.
/// Each entry: (rx_channel_number, tx_channel_name, tx_device_name).
pub fn cmd_add_subscriptions(subscriptions: &[(u16, &str, &str)], txn: u16) -> Result<Vec<u8>> {
    let count = subscriptions.len();
    if !(1..=16).contains(&count) {
        return Err(Error::Protocol(format!(
            "bulk subscription count must be 1-16, got {count}"
        )));
    }
    let packet_header_size = 8usize; // ARC header
    let first_record_size = 10usize;
    let additional_record_size = 6usize;
    let records_total = first_record_size + additional_record_size * (count - 1);
    let padding_size = 44usize.saturating_sub(records_total);
    let string_table_offset = packet_header_size + records_total + padding_size;

    let mut string_table: Vec<u8> = Vec::new();
    let mut records: Vec<(u16, u16, u16)> = Vec::with_capacity(count);

    for (rx_chan, tx_chan_name, tx_dev_name) in subscriptions {
        let tx_chan_abs = string_table_offset + string_table.len();
        string_table.extend_from_slice(tx_chan_name.as_bytes());
        string_table.push(0);
        let tx_dev_abs = string_table_offset + string_table.len();
        string_table.extend_from_slice(tx_dev_name.as_bytes());
        string_table.push(0);
        records.push((*rx_chan, tx_chan_abs as u16, tx_dev_abs as u16));
    }

    let mut payload: Vec<u8> =
        Vec::with_capacity(records_total + padding_size + string_table.len());
    let (rx0, tx_off0, dev_off0) = records[0];
    payload.extend_from_slice(&0u16.to_be_bytes());
    payload.extend_from_slice(&[0x02, count as u8, 0x00, rx0 as u8]);
    payload.extend_from_slice(&[(tx_off0 >> 8) as u8, (tx_off0 & 0xFF) as u8]);
    payload.extend_from_slice(&[(dev_off0 >> 8) as u8, (dev_off0 & 0xFF) as u8]);
    for (rx, tx_off, dev_off) in &records[1..] {
        payload.push(0);
        payload.push(*rx as u8);
        payload.extend_from_slice(&tx_off.to_be_bytes());
        payload.extend_from_slice(&dev_off.to_be_bytes());
    }
    payload.extend(std::iter::repeat_n(0u8, padding_size));
    payload.extend_from_slice(&string_table);

    Ok(DantePacket::build_request(
        OPCODE_SUBSCRIPTION_ADD,
        &payload,
        txn,
    ))
}

/// ARC: remove subscriptions for a list of RX channel numbers.
pub fn cmd_remove_subscriptions(rx_channels: &[u32], txn: u16) -> Result<Vec<u8>> {
    if rx_channels.is_empty() {
        return Err(Error::Protocol(
            "must specify at least one channel to unsubscribe".into(),
        ));
    }
    let mut payload = Vec::with_capacity(4 + rx_channels.len() * 4);
    payload.extend_from_slice(&(rx_channels.len() as u32).to_be_bytes());
    for ch in rx_channels {
        payload.extend_from_slice(&ch.to_be_bytes());
    }
    Ok(DantePacket::build_request(
        OPCODE_SUBSCRIPTION_REMOVE,
        &payload,
        txn,
    ))
}

/// ARC: set device latency (microseconds).
pub fn cmd_set_latency_us(latency_us: u32, txn: u16) -> Vec<u8> {
    let mut payload: Vec<u8> = Vec::with_capacity(40);
    payload.extend_from_slice(&0u16.to_be_bytes());
    payload.extend_from_slice(&[0x05, 0x03, 0x82, 0x05, 0x00]);
    payload.extend_from_slice(&[0x20, 0x02, 0x11, 0x00, 0x10, 0x83]);
    payload.extend_from_slice(&[0x01, 0x00, 0x24, 0x82, 0x19, 0x83]);
    payload.extend_from_slice(&[0x01, 0x83, 0x02, 0x83, 0x06]);
    let lat = latency_us.to_be_bytes();
    payload.extend_from_slice(&lat[1..]); // 3 bytes
    payload.push(0);
    payload.extend_from_slice(&lat[1..]);
    DantePacket::build_request(OPCODE_DEVICE_SETTINGS_SET, &payload, txn)
}

// ----- Settings-port commands (UDP/8700, PROTOCOL_SETTINGS header) ---------

fn pack_settings(payload: &[u8]) -> Vec<u8> {
    let total_length = (payload.len() + 4) as u8;
    let mut out = Vec::with_capacity(payload.len() + 4);
    out.extend_from_slice(&PROTOCOL_SETTINGS.to_be_bytes());
    out.push(0x00);
    out.push(total_length);
    out.extend_from_slice(payload);
    out
}

fn settings_payload(seq: u16, mac: &[u8; 6], magic_subop: u8, body: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(2 + 2 + 6 + 2 + 10 + body.len());
    p.extend_from_slice(&seq.to_be_bytes());
    p.extend_from_slice(&0u16.to_be_bytes());
    p.extend_from_slice(mac);
    p.extend_from_slice(&0u16.to_be_bytes());
    p.extend_from_slice(SETTINGS_MAGIC_BASE);
    p.push(0x07);
    p.push(magic_subop);
    p.extend_from_slice(body);
    p
}

/// Settings port: reboot the device.
pub fn cmd_reboot(host_mac: Option<[u8; 6]>) -> Vec<u8> {
    let mac = host_mac.unwrap_or([0; 6]);
    let mut body = Vec::new();
    body.extend_from_slice(&[0x00, 0x90, 0x00, 0x00, 0x00, 0x64]);
    body.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]);
    let payload = settings_payload(0x0000, &mac, 0x3a, &body);
    pack_settings(&payload)
}

/// Settings port: blink/identify the device.
pub fn cmd_identify() -> Vec<u8> {
    let mac = [0u8; 6];
    let body = [0x00, 0x63, 0x00, 0x00, 0x00, 0x64];
    let payload = settings_payload(0x0BC8, &mac, 0x31, &body);
    pack_settings(&payload)
}

/// Settings port: set sample rate (Hz, e.g. 48000, 96000).
pub fn cmd_set_sample_rate(sample_rate: u32) -> Vec<u8> {
    let mac = *b"RT\x00\x00\x00\x00";
    let mut body = Vec::with_capacity(20);
    body.extend_from_slice(&[0x00, 0x81, 0x00, 0x00, 0x00, 0x64]);
    body.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x00]);
    let sr = sample_rate.to_be_bytes();
    body.extend_from_slice(&sr[1..]);
    let payload = settings_payload(0x03D4, &mac, 0x27, &body);
    pack_settings(&payload)
}

/// Settings port: set encoding (bit depth code; e.g. 16, 24, 32).
pub fn cmd_set_encoding(encoding: u8) -> Vec<u8> {
    let mac = *b"RT\x00\x00\x00\x00";
    let mut body = Vec::with_capacity(20);
    body.extend_from_slice(&[0x00, 0x83, 0x00, 0x00, 0x00, 0x64]);
    body.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);
    body.push(encoding);
    let payload = settings_payload(0x03D7, &mac, 0x27, &body);
    pack_settings(&payload)
}

#[derive(Debug, Clone, Copy)]
pub enum GainDirection {
    Input,
    Output,
}

/// Settings port: set channel gain level (per-channel, vendor-specific scale).
pub fn cmd_set_gain_level(direction: GainDirection, channel_number: u8, gain_level: u8) -> Vec<u8> {
    let mac = *b"RT\x00\x00\x00\x00";
    let (seq, type_byte) = match direction {
        GainDirection::Input => (0x0344u16, 0x01u8),
        GainDirection::Output => (0x0326, 0x02),
    };
    let mut body = Vec::with_capacity(32);
    body.push(0x10);
    body.push(type_byte);
    body.extend_from_slice(&[0x0A, 0x00, 0x00, 0x00, 0x00, 0x00]);
    body.extend_from_slice(&[0x01, 0x00, 0x01, 0x00, 0x0C, 0x00]);
    body.push(0x10);
    body.push(type_byte);
    body.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00]);
    body.push(channel_number);
    body.extend_from_slice(&[0x00, 0x00, 0x00]);
    body.push(gain_level);
    let payload = settings_payload(seq, &mac, 0x27, &body);
    pack_settings(&payload)
}

/// Settings port: enable / disable AES67 mode.
pub fn cmd_enable_aes67(enabled: bool, host_mac: Option<[u8; 6]>) -> Vec<u8> {
    let mac = host_mac.unwrap_or([0; 6]);
    let mut body = Vec::with_capacity(12);
    body.extend_from_slice(&[0x10, 0x06, 0x00, 0x00, 0x00, 0x64]);
    body.extend_from_slice(&0x0001u16.to_be_bytes());
    body.extend_from_slice(&(if enabled { 0x0001u16 } else { 0x0000 }).to_be_bytes());
    let payload = settings_payload(0x22DC, &mac, 0x34, &body);
    pack_settings(&payload)
}

/// Settings port: probe AES67 status.
pub fn cmd_probe_aes67(host_mac: Option<[u8; 6]>, sequence: u16) -> Vec<u8> {
    let mac = host_mac.unwrap_or([0; 6]);
    let mut body = Vec::with_capacity(12);
    body.extend_from_slice(&[0x10, 0x06, 0x00, 0x00, 0x00, 0x64]);
    body.extend_from_slice(&0u16.to_be_bytes());
    body.extend_from_slice(&0u16.to_be_bytes());
    let payload = settings_payload(sequence, &mac, 0x3a, &body);
    pack_settings(&payload)
}

/// Settings port: set preferred-leader flag.
pub fn cmd_set_preferred_leader(
    is_preferred: bool,
    clock_source: u16,
    host_mac: Option<[u8; 6]>,
) -> Vec<u8> {
    let mac = host_mac.unwrap_or([0; 6]);
    let mut body = Vec::with_capacity(64);
    body.extend_from_slice(&[0x00, 0x21, 0x00, 0x00, 0x00, 0x64]);
    body.extend_from_slice(&0x0002u16.to_be_bytes());
    body.extend_from_slice(&clock_source.to_be_bytes());
    body.push(if is_preferred { 0x01 } else { 0x00 });
    body.extend(std::iter::repeat_n(0u8, 55));
    let payload = settings_payload(0x0021, &mac, 0x3a, &body);
    pack_settings(&payload)
}

/// Settings port: switch interface to DHCP.
pub fn cmd_set_interface_dhcp(host_mac: Option<[u8; 6]>) -> Vec<u8> {
    let mac = host_mac.unwrap_or([0; 6]);
    let mut body = Vec::with_capacity(40);
    body.extend_from_slice(&0x0013u16.to_be_bytes());
    body.extend_from_slice(&0x64u32.to_be_bytes());
    body.extend_from_slice(&[0x01, 0x1c, 0x00, 0x10]);
    body.extend(std::iter::repeat_n(0u8, 16));
    body.extend_from_slice(&[0x00, 0x02, 0x00, 0x00]);
    body.extend(std::iter::repeat_n(0u8, 4));
    let payload = settings_payload(0x0000, &mac, 0x3a, &body);
    pack_settings(&payload)
}

/// Settings port: switch interface to static IP config.
pub fn cmd_set_interface_static(
    ip: [u8; 4],
    netmask: [u8; 4],
    dns: [u8; 4],
    gateway: [u8; 4],
    host_mac: Option<[u8; 6]>,
) -> Vec<u8> {
    let mac = host_mac.unwrap_or([0; 6]);
    let mut body = Vec::with_capacity(48);
    body.extend_from_slice(&0x0013u16.to_be_bytes());
    body.extend_from_slice(&0x64u32.to_be_bytes());
    body.extend_from_slice(&[0x01, 0x1c, 0x0f, 0x10]);
    body.extend(std::iter::repeat_n(0u8, 4));
    body.extend_from_slice(&0x02u32.to_be_bytes());
    body.extend_from_slice(&ip);
    body.extend_from_slice(&netmask);
    body.extend_from_slice(&dns);
    body.extend_from_slice(&gateway);
    body.extend_from_slice(&[0x00, 0x02, 0x00, 0x00]);
    body.extend(std::iter::repeat_n(0u8, 4));
    let payload = settings_payload(0x0000, &mac, 0x3a, &body);
    pack_settings(&payload)
}

/// Settings port: probe interface status.
pub fn cmd_probe_interface_status(host_mac: Option<[u8; 6]>) -> Vec<u8> {
    let mac = host_mac.unwrap_or([0; 6]);
    let mut body = Vec::with_capacity(20);
    body.extend_from_slice(&0x0013u16.to_be_bytes());
    body.extend_from_slice(&0x64u32.to_be_bytes());
    body.extend(std::iter::repeat_n(0u8, 8));
    let payload = settings_payload(0x0000, &mac, 0x3a, &body);
    pack_settings(&payload)
}
