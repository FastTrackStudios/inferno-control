//! End-to-end test against an in-process virtual Dante device. Binds a
//! UDP socket on loopback, replays the captured fixtures from
//! `tests/fixtures/` in response to ARC opcodes, and drives the real
//! `DanteClient` against it. Closes the gap between unit-level parser
//! tests and full hardware integration (see #17).

use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::UdpSocket;

use inferno_net::protocol::DanteClient;

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
