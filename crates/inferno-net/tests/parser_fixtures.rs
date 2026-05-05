//! Parser regression tests against committed packet fixtures in
//! `tests/fixtures/`. Regenerate with:
//!   cargo run -p inferno-net --example gen_fixtures

use inferno_net::protocol::{
    parse_channel_count, parse_device_info, parse_device_settings, parse_rx_channels,
    parse_tx_channels, DantePacket,
};

fn load(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/{name}.bin", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

#[test]
fn channel_count_parses() {
    let bytes = load("channel_count");
    let resp = DantePacket::parse_response(&bytes).unwrap();
    let cc = parse_channel_count(&resp).unwrap();
    assert_eq!(cc.tx_count, 16);
    assert_eq!(cc.rx_count, 16);
    assert_eq!(cc.tx_active, 4);
    assert_eq!(cc.rx_active, 4);
    assert_eq!(cc.max_tx_flows, 32);
    assert_eq!(cc.max_rx_flows, 32);
}

#[test]
fn device_info_parses() {
    let bytes = load("device_info");
    let resp = DantePacket::parse_response(&bytes).unwrap();
    let info = parse_device_info(&resp).unwrap();
    assert_eq!(info.model_name, "AVIO-USB");
    assert_eq!(info.display_name, "Studio-AVIO");
    assert_eq!(info.model_code, "AVIOUSB");
    assert_eq!(info.port, "eth0");
}

#[test]
fn device_settings_parses() {
    let bytes = load("device_settings");
    let resp = DantePacket::parse_response(&bytes).unwrap();
    let s = parse_device_settings(&resp).unwrap();
    assert_eq!(s.sample_rate, Some(48000));
    assert_eq!(s.latency_us, Some(1_000_000));
    assert_eq!(s.min_latency_us, Some(150_000));
    assert_eq!(s.max_latency_us, Some(5_000_000));
}

#[test]
fn rx_channels_parses() {
    let bytes = load("rx_channels");
    let resp = DantePacket::parse_response(&bytes).unwrap();
    let chans = parse_rx_channels(&resp);
    assert_eq!(chans.len(), 2);
    assert_eq!(chans[0].number, 1);
    assert_eq!(chans[0].name, "01");
    assert_eq!(chans[0].tx_channel_name.as_deref(), Some("out-1"));
    assert_eq!(chans[0].tx_device_name.as_deref(), Some("AVIO-USB"));
    assert_eq!(chans[0].sample_rate, Some(48000));
    assert_eq!(chans[0].subscription_status, 0x0001);

    assert_eq!(chans[1].number, 2);
    assert_eq!(chans[1].name, "02");
    assert!(chans[1].tx_channel_name.is_none());
    assert!(chans[1].tx_device_name.is_none());
}

#[test]
fn tx_channels_parses() {
    let bytes = load("tx_channels");
    let resp = DantePacket::parse_response(&bytes).unwrap();
    let chans = parse_tx_channels(&resp);
    assert_eq!(chans.len(), 3);
    assert_eq!(chans[0].number, 1);
    assert_eq!(chans[0].name, "01");
    assert_eq!(chans[0].sample_rate, Some(48000));
    assert_eq!(chans[2].number, 3);
    assert_eq!(chans[2].name, "03");
}
