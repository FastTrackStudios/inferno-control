//! Generates synthetic ARC response fixtures used by `tests/parser_fixtures.rs`.
//!
//! Real captures from `tshark` / `command_query_*` should ideally replace
//! these — they are byte-faithful to the documented wire format and serve as
//! regression anchors until real hardware dumps are available.
//!
//! Run with: `cargo run -p inferno-net --example gen_fixtures`

use std::fs;
use std::path::PathBuf;

use inferno_net::protocol::consts::*;

fn header(opcode: u16, body: &[u8], txn: u16, result: u16) -> Vec<u8> {
    let length = 10u16 + body.len() as u16;
    let mut out = Vec::with_capacity(length as usize);
    out.extend_from_slice(&PROTOCOL_ID.to_be_bytes());
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(&txn.to_be_bytes());
    out.extend_from_slice(&opcode.to_be_bytes());
    out.extend_from_slice(&result.to_be_bytes());
    out.extend_from_slice(body);
    out
}

fn channel_count_body() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&0x0001u16.to_be_bytes()); // capability_flags
    b.extend_from_slice(&16u16.to_be_bytes()); // tx_count
    b.extend_from_slice(&16u16.to_be_bytes()); // rx_count
    b.extend_from_slice(&4u16.to_be_bytes()); // tx_active
    b.extend_from_slice(&4u16.to_be_bytes()); // rx_active
    b.extend_from_slice(&32u16.to_be_bytes()); // max_tx_flows
    b.extend_from_slice(&32u16.to_be_bytes()); // max_rx_flows
    b
}

fn device_info_body() -> Vec<u8> {
    // Body layout per parser::parse_device_info:
    //   off 6 -> code_ptr,  off 8  -> port_ptr,
    //   off 12 -> model_ptr, off 14 -> display_ptr.
    // Strings are referenced as absolute offsets relative to the *full packet*
    // (header is 10 bytes), so the parser subtracts 10 before indexing body.
    let header_len: u16 = 10;
    let fixed_len: u16 = 18;

    let model = b"AVIO-USB\0";
    let display = b"Studio-AVIO\0";
    let code = b"AVIOUSB\0";
    let port = b"eth0\0";

    let mut strings = Vec::new();
    let model_off = header_len + fixed_len + strings.len() as u16;
    strings.extend_from_slice(model);
    let display_off = header_len + fixed_len + strings.len() as u16;
    strings.extend_from_slice(display);
    let code_off = header_len + fixed_len + strings.len() as u16;
    strings.extend_from_slice(code);
    let port_off = header_len + fixed_len + strings.len() as u16;
    strings.extend_from_slice(port);

    let mut b = vec![0u8; fixed_len as usize];
    b[6..8].copy_from_slice(&code_off.to_be_bytes());
    b[8..10].copy_from_slice(&port_off.to_be_bytes());
    b[12..14].copy_from_slice(&model_off.to_be_bytes());
    b[14..16].copy_from_slice(&display_off.to_be_bytes());
    b.extend_from_slice(&strings);
    b
}

fn device_settings_body() -> Vec<u8> {
    // header byte, record_count, then 4-byte (info_code, value_ptr) records,
    // then the u32 values pointed to.
    let header_len: u16 = 10;
    let n_records: u8 = 4;
    let fixed_len: u16 = 2 + 4 * n_records as u16;

    let mut values = Vec::new();
    let sr_ptr = header_len + fixed_len + values.len() as u16;
    values.extend_from_slice(&48000u32.to_be_bytes());
    let lat_ptr = header_len + fixed_len + values.len() as u16;
    values.extend_from_slice(&1_000_000u32.to_be_bytes());
    let lat_min_ptr = header_len + fixed_len + values.len() as u16;
    values.extend_from_slice(&150_000u32.to_be_bytes());
    let lat_max_ptr = header_len + fixed_len + values.len() as u16;
    values.extend_from_slice(&5_000_000u32.to_be_bytes());

    let mut b = Vec::with_capacity(fixed_len as usize + values.len());
    b.push(0x00); // pad / version
    b.push(n_records);
    b.extend_from_slice(&DEVICE_SETTINGS_INFO_SAMPLE_RATE.to_be_bytes());
    b.extend_from_slice(&sr_ptr.to_be_bytes());
    b.extend_from_slice(&DEVICE_SETTINGS_INFO_LATENCY.to_be_bytes());
    b.extend_from_slice(&lat_ptr.to_be_bytes());
    b.extend_from_slice(&DEVICE_SETTINGS_INFO_LATENCY_MIN.to_be_bytes());
    b.extend_from_slice(&lat_min_ptr.to_be_bytes());
    b.extend_from_slice(&DEVICE_SETTINGS_INFO_LATENCY_MAX.to_be_bytes());
    b.extend_from_slice(&lat_max_ptr.to_be_bytes());
    b.extend_from_slice(&values);
    b
}

fn rx_channels_body() -> Vec<u8> {
    // 2-byte header + N x 20-byte records. Pointers are absolute (packet-relative).
    const RECORD: u16 = 20;
    let header_len: u16 = 10;
    let body_header: u16 = 2;
    let n: u16 = 2;
    let fixed_len: u16 = body_header + n * RECORD;

    let mut strings = Vec::<u8>::new();
    let push = |s: &[u8], strings: &mut Vec<u8>| -> u16 {
        let off = header_len + fixed_len + strings.len() as u16;
        strings.extend_from_slice(s);
        strings.push(0);
        off
    };
    let sr1_off = header_len + fixed_len + strings.len() as u16;
    strings.extend_from_slice(&48000u32.to_be_bytes());
    let rx1_name = push(b"01", &mut strings);
    let tx1_chan = push(b"out-1", &mut strings);
    let tx1_dev = push(b"AVIO-USB", &mut strings);

    let sr2_off = header_len + fixed_len + strings.len() as u16;
    strings.extend_from_slice(&48000u32.to_be_bytes());
    let rx2_name = push(b"02", &mut strings);

    let mut b = vec![0u8; fixed_len as usize];
    b[0] = 0;
    b[1] = n as u8;

    let write = |slot: usize, bytes: &[u8], buf: &mut Vec<u8>| {
        buf[slot..slot + bytes.len()].copy_from_slice(bytes);
    };

    let r0 = body_header as usize;
    write(r0, &1u16.to_be_bytes(), &mut b);
    write(r0 + 2, &0u16.to_be_bytes(), &mut b);
    write(r0 + 4, &sr1_off.to_be_bytes(), &mut b);
    write(r0 + 6, &tx1_chan.to_be_bytes(), &mut b);
    write(r0 + 8, &tx1_dev.to_be_bytes(), &mut b);
    write(r0 + 10, &rx1_name.to_be_bytes(), &mut b);
    write(r0 + 12, &0x0009u16.to_be_bytes(), &mut b); // status
    write(r0 + 14, &0x0001u16.to_be_bytes(), &mut b); // sub_status

    let r1 = r0 + RECORD as usize;
    write(r1, &2u16.to_be_bytes(), &mut b);
    write(r1 + 2, &0u16.to_be_bytes(), &mut b);
    write(r1 + 4, &sr2_off.to_be_bytes(), &mut b);
    write(r1 + 6, &0u16.to_be_bytes(), &mut b);
    write(r1 + 8, &0u16.to_be_bytes(), &mut b);
    write(r1 + 10, &rx2_name.to_be_bytes(), &mut b);
    write(r1 + 12, &0u16.to_be_bytes(), &mut b);
    write(r1 + 14, &0u16.to_be_bytes(), &mut b);

    b.extend_from_slice(&strings);
    b
}

fn tx_channels_body() -> Vec<u8> {
    const RECORD: u16 = 8;
    let header_len: u16 = 10;
    let body_header: u16 = 2;
    let n: u16 = 3;
    let fixed_len: u16 = body_header + n * RECORD;

    let mut strings = Vec::<u8>::new();
    let mut sr_offs = Vec::new();
    for _ in 0..n {
        sr_offs.push(header_len + fixed_len + strings.len() as u16);
        strings.extend_from_slice(&48000u32.to_be_bytes());
    }
    let mut name_offs = Vec::new();
    for name in [b"01" as &[u8], b"02", b"03"] {
        name_offs.push(header_len + fixed_len + strings.len() as u16);
        strings.extend_from_slice(name);
        strings.push(0);
    }

    let mut b = vec![0u8; fixed_len as usize];
    b[0] = 0;
    b[1] = n as u8;
    for i in 0..n as usize {
        let off = body_header as usize + i * RECORD as usize;
        b[off..off + 2].copy_from_slice(&((i as u16) + 1).to_be_bytes());
        b[off + 2..off + 4].copy_from_slice(&sr_offs[i].to_be_bytes());
        b[off + 4..off + 6].copy_from_slice(&0u16.to_be_bytes()); // group
        b[off + 6..off + 8].copy_from_slice(&name_offs[i].to_be_bytes());
    }
    b.extend_from_slice(&strings);
    b
}

fn main() -> std::io::Result<()> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures");
    fs::create_dir_all(&dir)?;

    let cases: Vec<(&str, u16, Vec<u8>, u16)> = vec![
        (
            "channel_count",
            OPCODE_CHANNEL_COUNT,
            channel_count_body(),
            RESULT_CODE_SUCCESS,
        ),
        (
            "device_info",
            OPCODE_DEVICE_INFO,
            device_info_body(),
            RESULT_CODE_SUCCESS,
        ),
        (
            "device_settings",
            OPCODE_DEVICE_SETTINGS,
            device_settings_body(),
            RESULT_CODE_SUCCESS,
        ),
        (
            "rx_channels",
            OPCODE_RX_CHANNELS,
            rx_channels_body(),
            RESULT_CODE_SUCCESS_EXTENDED,
        ),
        (
            "tx_channels",
            OPCODE_TX_CHANNEL_INFO,
            tx_channels_body(),
            RESULT_CODE_SUCCESS_EXTENDED,
        ),
    ];

    for (name, opcode, body, result) in cases {
        let bytes = header(opcode, &body, 0xBEEF, result);
        let path = dir.join(format!("{name}.bin"));
        fs::write(&path, &bytes)?;
        println!("wrote {} ({} bytes)", path.display(), bytes.len());
    }
    Ok(())
}
