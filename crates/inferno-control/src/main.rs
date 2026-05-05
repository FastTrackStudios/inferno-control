use std::io::{BufRead, BufReader};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use tracing_subscriber::EnvFilter;

use inferno_net::aliases::Aliases;
use inferno_net::protocol::commands::{ChannelDirection, GainDirection};
use inferno_net::protocol::{DanteClient, DEVICE_ARC_PORT};

#[derive(Parser, Debug)]
#[command(
    name = "inferno-control",
    version,
    about = "Control Dante/Inferno network audio devices"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,

    /// Discovery / request timeout in seconds
    #[arg(long, default_value_t = 2)]
    timeout: u64,

    /// Optional TOML file with extra `[[alias]]` rules; overrides bundled
    /// defaults. Falls back to $INFERNO_ALIASES if unset.
    #[arg(long, global = true, env = "INFERNO_ALIASES")]
    aliases: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    Device {
        #[command(subcommand)]
        op: DeviceOp,
    },
    Channel {
        #[command(subcommand)]
        op: ChannelOp,
    },
    Subscription {
        #[command(subcommand)]
        op: SubscriptionOp,
    },
    Config {
        #[command(subcommand)]
        op: ConfigOp,
    },
    /// Probe a device and dump everything we know about it (name, info,
    /// channel counts, settings, channels, subscriptions, per-query latency).
    /// Useful when filing bug reports against odd devices.
    Diagnose {
        ip: String,
        /// ARC port override (defaults to 4440 / mDNS-advertised)
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        json: bool,
    },
    /// Multicast TX flow management.
    Flow {
        #[command(subcommand)]
        op: FlowOp,
    },
}

#[derive(Subcommand, Debug)]
enum FlowOp {
    /// List active TX flows on the device. Auto-detects which flow
    /// protocol id the device speaks unless `--protocol` is given.
    List {
        ip: String,
        /// Force a specific flow protocol id (0x2729, 0x2801, or 0x2809).
        /// Hex with or without 0x prefix.
        #[arg(long, value_parser = parse_hex_u16)]
        protocol: Option<u16>,
        #[arg(long)]
        json: bool,
    },
}

fn parse_hex_u16(s: &str) -> Result<u16, String> {
    let trimmed = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    u16::from_str_radix(trimmed, 16).map_err(|e| format!("invalid hex u16 `{s}`: {e}"))
}

#[derive(Subcommand, Debug)]
enum DeviceOp {
    /// Discover devices on the network
    List {
        #[arg(long)]
        json: bool,
        /// Query each device for model + channel counts + sample rate
        #[arg(long)]
        details: bool,
    },
    /// Show detailed info for a single device
    Info {
        ip: String,
        /// ARC port override (defaults to 4440; some Inferno-AoIP devices
        /// advertise non-standard ports such as 4410)
        #[arg(long)]
        port: Option<u16>,
    },
    /// Rename device
    SetName { ip: String, name: String },
    /// Reset device name to factory default
    ResetName { ip: String },
    /// Reboot device
    Reboot { ip: String },
    /// Identify (blink) device
    Identify { ip: String },
    /// Probe whether the device is administratively locked.
    /// Sends a privileged ARC write and inspects result_code (0x0600 =
    /// locked, 0x0001 = unlocked, anything else = unknown).
    LockState { ip: String },
}

#[derive(Subcommand, Debug)]
enum ChannelOp {
    /// List TX + RX channels
    List {
        ip: String,
        #[arg(long)]
        json: bool,
    },
    /// Set channel name
    SetName {
        ip: String,
        #[arg(value_enum)]
        direction: Direction,
        channel: u8,
        name: String,
    },
    /// Reset channel name to factory default
    ResetName {
        ip: String,
        #[arg(value_enum)]
        direction: Direction,
        channel: u8,
    },
    /// Set channel gain
    SetGain {
        ip: String,
        #[arg(value_enum)]
        direction: GainDir,
        channel: u8,
        gain: u8,
    },
}

#[derive(Subcommand, Debug)]
enum SubscriptionOp {
    /// List active RX subscriptions
    List {
        ip: String,
        #[arg(long)]
        json: bool,
    },
    /// Add a subscription: rx_channel <- tx_channel@tx_device
    Add {
        ip: String,
        rx_channel: u16,
        tx_channel: String,
        tx_device: String,
    },
    /// Remove subscription on rx_channel
    Remove { ip: String, rx_channel: u32 },
    /// Bulk-add subscriptions from CSV (rx_channel,tx_channel,tx_device per line).
    /// Reads from FILE, or stdin when FILE is omitted or `-`. Lines beginning
    /// with `#` and blank lines are skipped. Records are batched 16 per packet.
    BulkAdd {
        ip: String,
        /// CSV file; omit or pass `-` to read from stdin
        file: Option<PathBuf>,
        /// Continue after individual batch failures
        #[arg(long)]
        keep_going: bool,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigOp {
    /// Set sample rate (Hz, e.g. 48000, 96000)
    SetSampleRate { ip: String, hz: u32 },
    /// Set encoding (e.g. 16, 24, 32)
    SetEncoding { ip: String, bits: u8 },
    /// Set device latency (microseconds)
    SetLatency { ip: String, us: u32 },
    /// Enable/disable AES67 mode
    Aes67 { ip: String, enabled: bool },
    /// Set preferred leader flag
    PreferredLeader {
        ip: String,
        enabled: bool,
        #[arg(long, default_value_t = 0)]
        clock_source: u16,
    },
    /// Switch interface to DHCP
    Dhcp { ip: String },
    /// Switch interface to static IP
    Static {
        ip: String,
        new_ip: IpAddr,
        netmask: IpAddr,
        dns: IpAddr,
        gateway: IpAddr,
    },
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum Direction {
    Tx,
    Rx,
}

impl From<Direction> for ChannelDirection {
    fn from(d: Direction) -> Self {
        match d {
            Direction::Tx => ChannelDirection::Tx,
            Direction::Rx => ChannelDirection::Rx,
        }
    }
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum GainDir {
    Input,
    Output,
}

impl From<GainDir> for GainDirection {
    fn from(d: GainDir) -> Self {
        match d {
            GainDir::Input => GainDirection::Input,
            GainDir::Output => GainDirection::Output,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let timeout = Duration::from_secs(cli.timeout);
    let aliases = Aliases::load_with_overrides(cli.aliases.as_deref())?;

    match cli.cmd {
        Cmd::Device { op } => match op {
            DeviceOp::List { json, details } => {
                device_list(timeout, json, details, &aliases).await?
            }
            DeviceOp::Info { ip, port } => device_info(ip, port, timeout, &aliases).await?,
            DeviceOp::SetName { ip, name } => {
                client(&ip, timeout)?.set_device_name(&name).await?;
                println!("ok");
            }
            DeviceOp::ResetName { ip } => {
                client(&ip, timeout)?.reset_device_name().await?;
                println!("ok");
            }
            DeviceOp::Reboot { ip } => {
                client(&ip, timeout)?.reboot(None).await?;
                println!("reboot sent (no response expected)");
            }
            DeviceOp::Identify { ip } => {
                client(&ip, timeout)?.identify().await?;
                println!("identify sent (no response expected)");
            }
            DeviceOp::LockState { ip } => match client(&ip, timeout)?.lock_state().await? {
                Some(true) => println!("locked"),
                Some(false) => println!("unlocked"),
                None => println!("unknown (unexpected result_code)"),
            },
        },
        Cmd::Channel { op } => match op {
            ChannelOp::List { ip, json } => channel_list(ip, timeout, json).await?,
            ChannelOp::SetName {
                ip,
                direction,
                channel,
                name,
            } => {
                client(&ip, timeout)?
                    .set_channel_name(direction.into(), channel, &name)
                    .await?;
                println!("ok");
            }
            ChannelOp::ResetName {
                ip,
                direction,
                channel,
            } => {
                client(&ip, timeout)?
                    .reset_channel_name(direction.into(), channel)
                    .await?;
                println!("ok");
            }
            ChannelOp::SetGain {
                ip,
                direction,
                channel,
                gain,
            } => {
                client(&ip, timeout)?
                    .set_gain_level(direction.into(), channel, gain)
                    .await?;
                println!("gain command sent (no response expected)");
            }
        },
        Cmd::Subscription { op } => match op {
            SubscriptionOp::List { ip, json } => subscription_list(ip, timeout, json).await?,
            SubscriptionOp::Add {
                ip,
                rx_channel,
                tx_channel,
                tx_device,
            } => {
                client(&ip, timeout)?
                    .add_subscription(rx_channel, &tx_channel, &tx_device)
                    .await?;
                println!("ok");
            }
            SubscriptionOp::Remove { ip, rx_channel } => {
                client(&ip, timeout)?
                    .remove_subscription(rx_channel)
                    .await?;
                println!("ok");
            }
            SubscriptionOp::BulkAdd {
                ip,
                file,
                keep_going,
            } => {
                subscription_bulk_add(ip, file, keep_going, timeout).await?;
            }
        },
        Cmd::Diagnose { ip, port, json } => diagnose(ip, port, timeout, json, &aliases).await?,
        Cmd::Flow { op } => match op {
            FlowOp::List { ip, protocol, json } => flow_list(ip, protocol, timeout, json).await?,
        },
        Cmd::Config { op } => match op {
            ConfigOp::SetSampleRate { ip, hz } => {
                client(&ip, timeout)?.set_sample_rate(hz).await?;
                println!("sample-rate command sent");
            }
            ConfigOp::SetEncoding { ip, bits } => {
                client(&ip, timeout)?.set_encoding(bits).await?;
                println!("encoding command sent");
            }
            ConfigOp::SetLatency { ip, us } => {
                client(&ip, timeout)?.set_latency_us(us).await?;
                println!("ok");
            }
            ConfigOp::Aes67 { ip, enabled } => {
                client(&ip, timeout)?.enable_aes67(enabled, None).await?;
                println!("aes67 command sent");
            }
            ConfigOp::PreferredLeader {
                ip,
                enabled,
                clock_source,
            } => {
                client(&ip, timeout)?
                    .set_preferred_leader(enabled, clock_source, None)
                    .await?;
                println!("preferred-leader command sent");
            }
            ConfigOp::Dhcp { ip } => {
                client(&ip, timeout)?.set_interface_dhcp(None).await?;
                println!("dhcp command sent");
            }
            ConfigOp::Static {
                ip,
                new_ip,
                netmask,
                dns,
                gateway,
            } => {
                client(&ip, timeout)?
                    .set_interface_static(new_ip, netmask, dns, gateway, None)
                    .await?;
                println!("static-ip command sent");
            }
        },
    }
    Ok(())
}

fn arc_addr(ip: &str) -> Result<SocketAddr> {
    format!("{}:{}", ip, DEVICE_ARC_PORT)
        .parse()
        .map_err(|e| anyhow!("invalid IP {ip}: {e}"))
}

fn client(ip: &str, timeout: Duration) -> Result<DanteClient> {
    Ok(DanteClient::new(arc_addr(ip)?, timeout))
}

async fn device_list(
    timeout: Duration,
    json: bool,
    details: bool,
    aliases: &Aliases,
) -> Result<()> {
    let devices = inferno_net::discovery::browse(timeout).await?;
    let mut list: Vec<_> = devices.into_values().collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));

    if !details {
        if json {
            let with_alias: Vec<_> = list
                .iter()
                .map(|d| {
                    serde_json::json!({
                        "name": d.name,
                        "alias": aliases.lookup(&[&d.name]),
                        "addresses": d.addresses.iter().map(|a| a.to_string()).collect::<Vec<_>>(),
                        "mdns_channel_count": d.mdns_channel_names.len(),
                        "mdns_channel_names": d.mdns_channel_names,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&with_alias)?);
        } else {
            for d in &list {
                let addrs = d
                    .addresses
                    .iter()
                    .map(|a| a.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                let alias = aliases
                    .lookup(&[&d.name])
                    .map(|a| format!("  [{a}]"))
                    .unwrap_or_default();
                let chan_hint = if d.mdns_channel_names.is_empty() {
                    String::new()
                } else {
                    format!("  ({} chan)", d.mdns_channel_names.len())
                };
                println!("{:<32} {}{}{}", d.name, addrs, alias, chan_hint);
            }
            println!("\n{} device(s)", list.len());
        }
        return Ok(());
    }

    let mut rows = Vec::new();
    for d in &list {
        let Some(ip) = d.addresses.iter().find(|a| a.is_ipv4()).copied() else {
            continue;
        };
        let arc_port = d.arc_port();
        let c = DanteClient::new(SocketAddr::new(ip, arc_port), timeout);
        let info = c.device_info().await.ok();
        let count = c.channel_count().await.ok();
        let settings = c.device_settings().await.ok();
        let model_name = info.as_ref().map(|i| i.model_name.as_str()).unwrap_or("");
        let model_code = info.as_ref().map(|i| i.model_code.as_str()).unwrap_or("");
        let alias = aliases.lookup(&[&d.name, model_name, model_code]);
        rows.push(serde_json::json!({
            "name": d.name,
            "alias": alias,
            "ip": ip.to_string(),
            "arc_port": arc_port,
            "model": info.as_ref().map(|i| &i.model_name),
            "tx_count": count.map(|c| c.tx_count),
            "rx_count": count.map(|c| c.rx_count),
            "sample_rate": settings.as_ref().and_then(|s| s.sample_rate),
            "latency_us": settings.as_ref().and_then(|s| s.latency_us),
        }));
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
    } else {
        println!(
            "{:<28} {:<16} {:<22} {:<24} {:>4}/{:<4} {:>6} {:>8}",
            "NAME", "IP", "ALIAS", "MODEL", "TX", "RX", "SR", "LAT_US"
        );
        for r in &rows {
            println!(
                "{:<28} {:<16} {:<22} {:<24} {:>4}/{:<4} {:>6} {:>8}",
                r["name"].as_str().unwrap_or(""),
                r["ip"].as_str().unwrap_or(""),
                r["alias"].as_str().unwrap_or("-"),
                r["model"].as_str().unwrap_or("-"),
                r["tx_count"]
                    .as_u64()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "-".into()),
                r["rx_count"]
                    .as_u64()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "-".into()),
                r["sample_rate"]
                    .as_u64()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "-".into()),
                r["latency_us"]
                    .as_u64()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "-".into()),
            );
        }
    }
    Ok(())
}

async fn device_info(
    ip: String,
    port: Option<u16>,
    timeout: Duration,
    aliases: &Aliases,
) -> Result<()> {
    let c = match port {
        Some(p) => DanteClient::new(format!("{ip}:{p}").parse()?, timeout),
        None => client(&ip, timeout)?,
    };
    let name = c.device_name().await?;
    let info = c.device_info().await?;
    let count = c.channel_count().await?;
    let settings = c.device_settings().await?;
    let alias = aliases.lookup(&[&name, &info.model_name, &info.model_code]);
    let payload = serde_json::json!({
        "ip": ip,
        "name": name,
        "alias": alias,
        "info": info,
        "channel_count": count,
        "settings": settings,
    });
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

async fn channel_list(ip: String, timeout: Duration, json: bool) -> Result<()> {
    let c = client(&ip, timeout)?;
    let tx = c.all_tx_channels().await?;
    let rx = c.all_rx_channels().await?;

    if json {
        let payload = serde_json::json!({ "tx": tx, "rx": rx });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    println!("TX channels ({}):", tx.len());
    for c in &tx {
        let sr = c
            .sample_rate
            .map(|s| s.to_string())
            .unwrap_or_else(|| "-".into());
        println!("  {:>3}  {:<24} sr={}", c.number, c.name, sr);
    }
    println!("\nRX channels ({}):", rx.len());
    for c in &rx {
        let sub = match (&c.tx_device_name, &c.tx_channel_name) {
            (Some(d), Some(t)) => format!("{}@{}", t, d),
            _ => "-".into(),
        };
        println!(
            "  {:>3}  {:<24}  sub={}  status={:#06x}",
            c.number, c.name, sub, c.subscription_status
        );
    }
    Ok(())
}

struct BulkRow {
    line_no: usize,
    rx_channel: u16,
    tx_channel: String,
    tx_device: String,
}

fn parse_bulk_rows<R: BufRead>(reader: R) -> Result<Vec<BulkRow>> {
    let mut out = Vec::new();
    for (i, line) in reader.lines().enumerate() {
        let line_no = i + 1;
        let line = line.with_context(|| format!("read line {line_no}"))?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = trimmed.splitn(3, ',').map(str::trim).collect();
        if parts.len() != 3 {
            return Err(anyhow!(
                "line {line_no}: expected `rx_channel,tx_channel,tx_device`, got `{trimmed}`"
            ));
        }
        let rx_channel: u16 = parts[0]
            .parse()
            .with_context(|| format!("line {line_no}: invalid rx_channel `{}`", parts[0]))?;
        if parts[1].is_empty() || parts[2].is_empty() {
            return Err(anyhow!(
                "line {line_no}: tx_channel and tx_device cannot be empty"
            ));
        }
        out.push(BulkRow {
            line_no,
            rx_channel,
            tx_channel: parts[1].to_string(),
            tx_device: parts[2].to_string(),
        });
    }
    Ok(out)
}

async fn subscription_bulk_add(
    ip: String,
    file: Option<PathBuf>,
    keep_going: bool,
    timeout: Duration,
) -> Result<()> {
    let rows = match file.as_deref() {
        None => parse_bulk_rows(BufReader::new(std::io::stdin().lock()))?,
        Some(p) if p.as_os_str() == "-" => {
            parse_bulk_rows(BufReader::new(std::io::stdin().lock()))?
        }
        Some(p) => {
            let f = std::fs::File::open(p).with_context(|| format!("open {}", p.display()))?;
            parse_bulk_rows(BufReader::new(f))?
        }
    };

    if rows.is_empty() {
        println!("no subscription rows to add");
        return Ok(());
    }

    let c = client(&ip, timeout)?;
    let mut sent = 0usize;
    let mut failed = 0usize;
    for chunk in rows.chunks(16) {
        let pairs: Vec<(u16, &str, &str)> = chunk
            .iter()
            .map(|r| (r.rx_channel, r.tx_channel.as_str(), r.tx_device.as_str()))
            .collect();
        let first = chunk.first().map(|r| r.line_no).unwrap_or(0);
        let last = chunk.last().map(|r| r.line_no).unwrap_or(0);
        match c.add_subscriptions_bulk(&pairs).await {
            Ok(()) => {
                sent += chunk.len();
                println!("ok: lines {first}-{last} ({} subs)", chunk.len());
            }
            Err(e) => {
                failed += chunk.len();
                eprintln!("error: lines {first}-{last}: {e}");
                if !keep_going {
                    return Err(e.into());
                }
            }
        }
    }
    println!("{sent} added, {failed} failed");
    if failed > 0 && !keep_going {
        return Err(anyhow!("{failed} subscription(s) failed"));
    }
    Ok(())
}

async fn subscription_list(ip: String, timeout: Duration, json: bool) -> Result<()> {
    let c = client(&ip, timeout)?;
    let rx = c.all_rx_channels().await?;
    let subs: Vec<_> = rx
        .into_iter()
        .filter(|c| c.tx_device_name.is_some() || c.tx_channel_name.is_some())
        .collect();

    if json {
        println!("{}", serde_json::to_string_pretty(&subs)?);
    } else {
        for c in &subs {
            let tx_dev = c.tx_device_name.as_deref().unwrap_or("-");
            let tx_ch = c.tx_channel_name.as_deref().unwrap_or("-");
            println!(
                "{:<24} <- {}@{}   status={:#06x}",
                c.name, tx_ch, tx_dev, c.subscription_status
            );
        }
    }
    Ok(())
}

async fn diagnose(
    ip: String,
    port: Option<u16>,
    timeout: Duration,
    json: bool,
    aliases: &Aliases,
) -> Result<()> {
    let addr: SocketAddr = match port {
        Some(p) => format!("{ip}:{p}").parse()?,
        None => arc_addr(&ip)?,
    };
    let c = DanteClient::new(addr, timeout);

    let mut timings = serde_json::Map::new();
    let mut warnings: Vec<String> = Vec::new();

    macro_rules! timed {
        ($label:expr, $fut:expr) => {{
            let t = Instant::now();
            let r = $fut.await;
            timings.insert(
                $label.to_string(),
                serde_json::json!(t.elapsed().as_micros() as u64),
            );
            r
        }};
    }

    let name = timed!("device_name", c.device_name()).ok();
    let info = timed!("device_info", c.device_info()).ok();
    let count = timed!("channel_count", c.channel_count()).ok();
    let settings = timed!("device_settings", c.device_settings()).ok();
    let tx = timed!("all_tx_channels", c.all_tx_channels()).unwrap_or_default();
    let rx = timed!("all_rx_channels", c.all_rx_channels()).unwrap_or_default();

    if name.is_none() {
        warnings.push("device_name query failed".into());
    }
    if info.is_none() {
        warnings.push("device_info query failed".into());
    }
    if count.is_none() {
        warnings.push("channel_count query failed".into());
    }
    if settings.as_ref().and_then(|s| s.sample_rate).is_none() {
        warnings.push("no sample_rate reported".into());
    }
    let unhealthy_subs: Vec<_> = rx
        .iter()
        .filter(|c| {
            (c.tx_device_name.is_some() || c.tx_channel_name.is_some())
                && c.subscription_status != 0x0001
        })
        .map(|c| {
            format!(
                "rx[{}] {} <- {}@{} status={:#06x}",
                c.number,
                c.name,
                c.tx_channel_name.as_deref().unwrap_or("-"),
                c.tx_device_name.as_deref().unwrap_or("-"),
                c.subscription_status
            )
        })
        .collect();
    if !unhealthy_subs.is_empty() {
        warnings.push(format!(
            "{} subscription(s) not in healthy state (0x0001)",
            unhealthy_subs.len()
        ));
    }

    let alias = match (&name, &info) {
        (Some(n), Some(i)) => aliases.lookup(&[n, &i.model_name, &i.model_code]),
        (Some(n), None) => aliases.lookup(&[n]),
        _ => None,
    };

    let payload = serde_json::json!({
        "ip": ip,
        "addr": addr.to_string(),
        "name": name,
        "alias": alias,
        "info": info,
        "channel_count": count,
        "settings": settings,
        "tx_channels": tx,
        "rx_channels": rx,
        "timings_us": timings,
        "warnings": warnings,
        "unhealthy_subscriptions": unhealthy_subs,
    });

    if json {
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    println!("address       : {addr}");
    if let Some(n) = &name {
        println!("device name   : {n}");
    }
    if let Some(a) = &alias {
        println!("alias         : {a}");
    }
    if let Some(i) = &info {
        println!(
            "model         : {} ({}) port={}",
            i.model_name, i.model_code, i.port
        );
    }
    if let Some(c) = &count {
        println!(
            "channels      : tx={}/{} rx={}/{} max_flows tx={} rx={}",
            c.tx_active, c.tx_count, c.rx_active, c.rx_count, c.max_tx_flows, c.max_rx_flows
        );
    }
    if let Some(s) = &settings {
        println!(
            "settings      : sr={:?} latency_us={:?} (min={:?}, max={:?})",
            s.sample_rate, s.latency_us, s.min_latency_us, s.max_latency_us
        );
    }
    println!("tx channels   : {}", tx.len());
    println!("rx channels   : {}", rx.len());
    let active_subs = rx
        .iter()
        .filter(|c| c.tx_device_name.is_some() || c.tx_channel_name.is_some())
        .count();
    println!("subscriptions : {active_subs} active");
    println!("\nquery timings (us):");
    for (k, v) in &timings {
        println!("  {:<16} {}", k, v.as_u64().unwrap_or(0));
    }
    if !warnings.is_empty() {
        println!("\nwarnings:");
        for w in &warnings {
            println!("  - {w}");
        }
    }
    if !unhealthy_subs.is_empty() {
        println!("\nunhealthy subscriptions:");
        for s in &unhealthy_subs {
            println!("  - {s}");
        }
    }
    Ok(())
}

async fn flow_list(ip: String, protocol: Option<u16>, timeout: Duration, json: bool) -> Result<()> {
    let c = client(&ip, timeout)?;
    let (protocol_id, flows) = c.query_tx_flows(protocol).await?;
    if json {
        let payload = serde_json::json!({
            "flow_protocol_id": format!("0x{protocol_id:04X}"),
            "flows": flows,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }
    println!("flow protocol : 0x{protocol_id:04X}");
    println!("active flows  : {}", flows.len());
    for f in &flows {
        let kind = if f.is_multicast() {
            "multicast".to_string()
        } else {
            format!("0x{:04X}", f.flow_type)
        };
        println!(
            "  flow #{:<3} type={:<10} sr={:<6} enc={:<3} fpp={:<3} channels={:?}",
            f.flow_number, kind, f.sample_rate, f.encoding, f.fpp, f.channels
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_bulk_rows;

    #[test]
    fn parses_csv_skipping_comments_and_blanks() {
        let csv = "# header\n\n1, out-1 , AVIO-USB\n2,out-2,AVIO-USB\n";
        let rows = parse_bulk_rows(csv.as_bytes()).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].rx_channel, 1);
        assert_eq!(rows[0].tx_channel, "out-1");
        assert_eq!(rows[0].tx_device, "AVIO-USB");
        assert_eq!(rows[1].rx_channel, 2);
    }

    #[test]
    fn rejects_bad_rx_channel() {
        assert!(parse_bulk_rows("foo,bar,baz\n".as_bytes()).is_err());
    }

    #[test]
    fn rejects_short_row() {
        assert!(parse_bulk_rows("1,only-two\n".as_bytes()).is_err());
    }

    #[test]
    fn rejects_empty_field() {
        assert!(parse_bulk_rows("1,,AVIO\n".as_bytes()).is_err());
    }
}
