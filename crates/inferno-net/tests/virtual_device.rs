//! End-to-end test against an in-process virtual Dante device. Binds a
//! UDP socket on loopback, replays the captured fixtures from
//! `tests/fixtures/` in response to ARC opcodes, and drives the real
//! `DanteClient` against it. Closes the gap between unit-level parser
//! tests and full hardware integration (see #17).

use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::UdpSocket;

use inferno_net::protocol::{DanteClient, DantePacket};

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/{name}.bin", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

/// Reply with the fixture bytes but rewrite the txn_id (bytes 4..6) to
/// match the request, so the client's correlation logic stays sane even
/// though it currently doesn't enforce it.
fn rewrite_txn(fixture_bytes: &mut [u8], req: &[u8]) {
    if fixture_bytes.len() >= 6 && req.len() >= 6 {
        fixture_bytes[4] = req[4];
        fixture_bytes[5] = req[5];
    }
}

async fn spawn_virtual_device() -> SocketAddr {
    let sock = UdpSocket::bind("127.0.0.1:0").await.expect("bind");
    let addr = sock.local_addr().expect("local_addr");

    tokio::spawn(async move {
        let mut buf = vec![0u8; 4096];
        loop {
            let (n, peer) = match sock.recv_from(&mut buf).await {
                Ok(v) => v,
                Err(_) => break,
            };
            let req = &buf[..n];
            if req.len() < 8 {
                continue;
            }
            let opcode = u16::from_be_bytes([req[6], req[7]]);
            let name = match opcode {
                0x1000 => "channel_count",
                0x1003 => "device_info",
                0x1100 => "device_settings",
                0x3000 => "rx_channels",
                0x2000 => "tx_channels",
                _ => continue,
            };
            let mut resp = fixture(name);
            rewrite_txn(&mut resp, req);
            let _ = sock.send_to(&resp, peer).await;
        }
    });

    addr
}

#[tokio::test]
async fn end_to_end_against_virtual_device() {
    let addr = spawn_virtual_device().await;
    let client = DanteClient::new(addr, Duration::from_secs(2));

    let count = client.channel_count().await.expect("channel_count");
    assert_eq!(count.tx_count, 16);
    assert_eq!(count.rx_count, 16);

    let info = client.device_info().await.expect("device_info");
    assert_eq!(info.model_name, "AVIO-USB");
    assert_eq!(info.display_name, "Studio-AVIO");

    let settings = client.device_settings().await.expect("device_settings");
    assert_eq!(settings.sample_rate, Some(48_000));
    assert_eq!(settings.latency_us, Some(1_000_000));

    let rx = client.rx_channels_page(0).await.expect("rx_channels_page");
    assert_eq!(rx.len(), 2);
    assert_eq!(rx[0].name, "01");

    let tx = client.tx_channels_page(0).await.expect("tx_channels_page");
    assert_eq!(tx.len(), 3);
}

/// A virtual device that:
/// - rejects flow queries on protocol 0x2729 / 0x2801 (replies with
///   result_code 0x0022, "generic error"),
/// - accepts queries on 0x2809 with one active multicast flow,
/// - rejects `set_latency` writes with `RESULT_CODE_LOCK_REJECTION`
///   (i.e. simulates a locked device).
async fn spawn_flow_and_lock_device() -> SocketAddr {
    let sock = UdpSocket::bind("127.0.0.1:0").await.expect("bind");
    let addr = sock.local_addr().expect("local_addr");

    tokio::spawn(async move {
        let mut buf = vec![0u8; 4096];
        loop {
            let (n, peer) = match sock.recv_from(&mut buf).await {
                Ok(v) => v,
                Err(_) => break,
            };
            let req = &buf[..n];
            if req.len() < 8 {
                continue;
            }
            let protocol_id = u16::from_be_bytes([req[0], req[1]]);
            let txn = u16::from_be_bytes([req[4], req[5]]);
            let opcode = u16::from_be_bytes([req[6], req[7]]);

            // Locked-device probe: latency-set comes in as a normal ARC
            // request (PROTOCOL_ID 0x27FF, opcode OPCODE_DEVICE_SETTINGS_SET
            // = 0x1101). Reply with result_code 0x0600.
            if protocol_id == 0x27FF && opcode == 0x1101 {
                let mut resp = Vec::new();
                resp.extend_from_slice(&0x27FFu16.to_be_bytes());
                resp.extend_from_slice(&10u16.to_be_bytes());
                resp.extend_from_slice(&txn.to_be_bytes());
                resp.extend_from_slice(&opcode.to_be_bytes());
                resp.extend_from_slice(&0x0600u16.to_be_bytes());
                let _ = sock.send_to(&resp, peer).await;
                continue;
            }

            // Flow protocol detection / query.
            // Standard variants 0x2729/0x2801 → reply with error.
            if (protocol_id == 0x2729 || protocol_id == 0x2801) && opcode == 0x2200 {
                let mut resp = Vec::new();
                resp.extend_from_slice(&protocol_id.to_be_bytes());
                resp.extend_from_slice(&10u16.to_be_bytes());
                resp.extend_from_slice(&txn.to_be_bytes());
                resp.extend_from_slice(&opcode.to_be_bytes());
                resp.extend_from_slice(&0x0022u16.to_be_bytes());
                let _ = sock.send_to(&resp, peer).await;
                continue;
            }

            // 0x2809 variant → success + one multicast flow.
            if protocol_id == 0x2809 && opcode == 0x2600 {
                // Body layout matches `parse_tx_flows`:
                //   [max_slots u8][active_count u8]
                //   [flow_number u16][record_pointer u16]    (record points
                //                                            into the FULL packet)
                //   ... flow descriptor at record_pointer-10 in body ...
                let mut body: Vec<u8> = Vec::new();
                body.push(8); // max_flow_slots
                body.push(1); // active_count

                // Reserve room for the (flow_number, record_pointer) pair
                // and fill it in once we know where the descriptor sits.
                let header_pair_off = body.len();
                body.extend_from_slice(&[0u8; 4]);

                // Flow descriptor starts here. Layout per
                // `_parse_flow_record`:
                //   [+0..+2  ] (skipped)
                //   [+2..+4  ] flow_type    = 0x0002 (multicast)
                //   [+4..+8  ] sample_rate  = 48000
                //   [+8..+10 ] encoding     = 24
                //   [+10..+12] fpp          = 6
                //   [+12..+22] (skipped)
                //   [+22..+24] channel_count = 2
                //   [+24..+28] channels[2]  = [1, 2]
                let descriptor_offset = body.len();
                body.extend_from_slice(&0u16.to_be_bytes()); // +0
                body.extend_from_slice(&0x0002u16.to_be_bytes()); // +2 flow_type
                body.extend_from_slice(&48_000u32.to_be_bytes()); // +4 sample_rate
                body.extend_from_slice(&24u16.to_be_bytes()); // +8 encoding
                body.extend_from_slice(&6u16.to_be_bytes()); // +10 fpp
                body.extend_from_slice(&[0u8; 10]); // +12..+22
                body.extend_from_slice(&2u16.to_be_bytes()); // +22 channel_count
                body.extend_from_slice(&1u16.to_be_bytes()); // +24 ch1
                body.extend_from_slice(&2u16.to_be_bytes()); // +26 ch2

                // record_pointer is relative to the full 10-byte-headered
                // packet; body offset N → packet offset N+10.
                let record_pointer = (descriptor_offset + 10) as u16;
                body[header_pair_off..header_pair_off + 2].copy_from_slice(&7u16.to_be_bytes()); // flow_number
                body[header_pair_off + 2..header_pair_off + 4]
                    .copy_from_slice(&record_pointer.to_be_bytes());

                // 10-byte flow header: protocol, length, txn, opcode, status.
                let length = (10 + body.len()) as u16;
                let mut resp = Vec::new();
                resp.extend_from_slice(&protocol_id.to_be_bytes());
                resp.extend_from_slice(&length.to_be_bytes());
                resp.extend_from_slice(&txn.to_be_bytes());
                resp.extend_from_slice(&opcode.to_be_bytes());
                resp.extend_from_slice(&0x0001u16.to_be_bytes()); // status
                resp.extend(body);

                // Sanity-check our own packet via the parser.
                let _ = DantePacket::parse_response(&resp).expect("self-check");
                let _ = sock.send_to(&resp, peer).await;
                continue;
            }
        }
    });

    addr
}

#[tokio::test]
async fn flow_query_detects_2809_and_parses_one_multicast_flow() {
    let addr = spawn_flow_and_lock_device().await;
    let client = DanteClient::new(addr, Duration::from_secs(1));

    let (proto, flows) = client.query_tx_flows(None).await.expect("query_tx_flows");
    assert_eq!(proto, 0x2809);
    assert_eq!(flows.len(), 1);
    let f = &flows[0];
    assert_eq!(f.flow_number, 7);
    assert!(f.is_multicast());
    assert_eq!(f.sample_rate, 48_000);
    assert_eq!(f.encoding, 24);
    assert_eq!(f.fpp, 6);
    assert_eq!(f.channels, vec![1, 2]);
}

#[tokio::test]
async fn lock_state_detects_lock_rejection() {
    let addr = spawn_flow_and_lock_device().await;
    let client = DanteClient::new(addr, Duration::from_secs(1));
    let state = client.lock_state().await.expect("lock_state");
    assert_eq!(state, Some(true));
}

#[tokio::test]
async fn timeout_when_device_silent() {
    let sock = UdpSocket::bind("127.0.0.1:0").await.expect("bind");
    let addr = sock.local_addr().expect("local_addr");
    // Drop sock immediately so the OS port is closed; alternatively keep
    // it alive and never reply. Keeping it alive matches a half-broken
    // device better.
    std::mem::forget(sock);

    let client = DanteClient::new(addr, Duration::from_millis(150));
    let err = client.channel_count().await.expect_err("should time out");
    let msg = format!("{err}");
    assert!(
        msg.to_lowercase().contains("timeout") || msg.to_lowercase().contains("timed out"),
        "expected timeout error, got: {msg}"
    );
}
