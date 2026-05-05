//! Dante request/response packet codec.

use super::consts::PROTOCOL_ID;
use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct DanteResponse {
    pub protocol: u16,
    pub length: u16,
    pub transaction_id: u16,
    pub opcode: u16,
    pub result_code: u16,
    pub body: Vec<u8>,
}

pub struct DantePacket;

impl DantePacket {
    /// Build an ARC request. Header is 8 bytes; full opcode body follows.
    pub fn build_request(opcode: u16, payload: &[u8], transaction_id: u16) -> Vec<u8> {
        let length = 8u16 + payload.len() as u16;
        let mut out = Vec::with_capacity(length as usize);
        out.extend_from_slice(&PROTOCOL_ID.to_be_bytes());
        out.extend_from_slice(&length.to_be_bytes());
        out.extend_from_slice(&transaction_id.to_be_bytes());
        out.extend_from_slice(&opcode.to_be_bytes());
        out.extend_from_slice(payload);
        out
    }

    /// Build a flow-control request. Header is 10 bytes (vs 8 for normal
    /// ARC) — `[protocol_id, length, txn, opcode, 0x0000]` — and the
    /// protocol id is one of `FLOW_PROTOCOL_IDS`, not the standard
    /// `PROTOCOL_ID`. See `services/arc.py::_build_flow_packet` in netaudio.
    pub fn build_flow_request(
        protocol_id: u16,
        opcode: u16,
        body: &[u8],
        transaction_id: u16,
    ) -> Vec<u8> {
        let length = 10u16 + body.len() as u16;
        let mut out = Vec::with_capacity(length as usize);
        out.extend_from_slice(&protocol_id.to_be_bytes());
        out.extend_from_slice(&length.to_be_bytes());
        out.extend_from_slice(&transaction_id.to_be_bytes());
        out.extend_from_slice(&opcode.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(body);
        out
    }

    pub fn parse_response(data: &[u8]) -> Result<DanteResponse> {
        if data.len() < 10 {
            return Err(Error::Protocol(format!(
                "response too short: {} bytes",
                data.len()
            )));
        }
        let protocol = u16::from_be_bytes([data[0], data[1]]);
        let length = u16::from_be_bytes([data[2], data[3]]);
        let transaction_id = u16::from_be_bytes([data[4], data[5]]);
        let opcode = u16::from_be_bytes([data[6], data[7]]);
        let result_code = u16::from_be_bytes([data[8], data[9]]);
        let body = data.get(10..).unwrap_or(&[]).to_vec();
        Ok(DanteResponse {
            protocol,
            length,
            transaction_id,
            opcode,
            result_code,
            body,
        })
    }
}

/// Read a NUL-terminated UTF-8 string at `offset` within `data`.
/// Returns empty string on bounds errors (matches Python parser semantics).
pub(crate) fn read_cstr(data: &[u8], offset: usize) -> String {
    if offset >= data.len() {
        return String::new();
    }
    let end = data[offset..]
        .iter()
        .position(|&b| b == 0)
        .map(|p| offset + p)
        .unwrap_or(data.len());
    String::from_utf8_lossy(&data[offset..end]).into_owned()
}
