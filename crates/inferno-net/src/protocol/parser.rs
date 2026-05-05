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
        if channel_number == 0 {
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
        let sample_rate = u32_at_pointer(b, sample_rate_ptr).filter(|v| *v > 0);
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
        let sample_rate = u32_at_pointer(b, sample_rate_ptr).filter(|v| *v > 0);
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
