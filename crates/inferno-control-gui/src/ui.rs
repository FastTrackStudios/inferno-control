use dioxus::prelude::*;

use crate::state::{AppState, DeviceEntry};
use crate::{fetch_device, scan_network, toggle_subscription};

#[component]
pub fn AppShell(state: Signal<AppState>) -> Element {
    let scan = move |_| {
        if state.read().scanning {
            return;
        }
        state.write().scanning = true;
        spawn(async move {
            let devs = scan_network().await;
            let mut s = state.write();
            s.devices = devs;
            s.scanning = false;
            s.status = Some(crate::state::Status::ok(&format!(
                "Discovered {} device(s)",
                s.devices.len()
            )));
        });
    };

    rsx! {
        div { class: "h-screen flex flex-col bg-neutral-950 text-neutral-100",
            TopBar { state, on_scan: scan }
            div { class: "flex-1 overflow-auto",
                if state.read().devices.is_empty() {
                    EmptyState { scanning: state.read().scanning }
                } else {
                    RoutingMatrix { state }
                }
            }
            StatusBar { state }
        }
    }
}

#[component]
fn TopBar(state: Signal<AppState>, on_scan: EventHandler<MouseEvent>) -> Element {
    let scanning = state.read().scanning;
    rsx! {
        header { class: "flex items-center justify-between px-4 py-2 border-b border-neutral-800 bg-neutral-900",
            div { class: "flex items-center gap-3",
                h1 { class: "text-sm font-semibold tracking-wide", "Inferno Control" }
                span { class: "text-xs text-neutral-500", "routing matrix" }
            }
            div { class: "flex items-center gap-2",
                {
                    let live = state.read().live_updates;
                    rsx! {
                        label { class: "flex items-center gap-1.5 text-xs text-neutral-400 select-none cursor-pointer",
                            input {
                                r#type: "checkbox",
                                checked: live,
                                onchange: move |e| {
                                    state.write().live_updates = e.value() == "true";
                                },
                            }
                            "Live updates"
                        }
                    }
                }
                button {
                    class: "px-3 py-1 rounded-md bg-blue-600 hover:bg-blue-500 disabled:opacity-50 text-xs font-medium",
                    disabled: scanning,
                    onclick: move |e| on_scan.call(e),
                    if scanning { "Scanning…" } else { "Scan network" }
                }
            }
        }
    }
}

#[component]
fn StatusBar(state: Signal<AppState>) -> Element {
    let s = state.read();
    let class_ = if s.status.as_ref().map(|s| s.error).unwrap_or(false) {
        "text-red-400"
    } else {
        "text-neutral-400"
    };
    let msg = s
        .status
        .as_ref()
        .map(|s| s.message.clone())
        .unwrap_or_else(|| {
            format!(
                "{} device(s) · {} TX · {} RX",
                s.devices.len(),
                s.devices.iter().map(|d| d.tx_channels.len()).sum::<usize>(),
                s.devices.iter().map(|d| d.rx_channels.len()).sum::<usize>(),
            )
        });
    rsx! {
        footer { class: "px-4 py-1 border-t border-neutral-800 bg-neutral-900 text-xs {class_}",
            "{msg}"
        }
    }
}

#[component]
fn EmptyState(scanning: bool) -> Element {
    rsx! {
        div { class: "h-full grid place-items-center text-neutral-500 text-sm",
            if scanning { "Scanning network…" } else { "Click \"Scan network\" to discover Dante devices." }
        }
    }
}

// --- the matrix ------------------------------------------------------------

#[component]
fn RoutingMatrix(state: Signal<AppState>) -> Element {
    let s = state.read();
    let tx_filter = s.tx_filter.clone();
    let rx_filter = s.rx_filter.clone();

    // Visible TX columns: device + channels (after filter, after collapse).
    let mut tx_cols: Vec<(&DeviceEntry, Vec<&inferno_net::protocol::parser::TxChannel>)> =
        Vec::new();
    for d in &s.devices {
        let chans: Vec<_> = d.visible_tx(&tx_filter).collect();
        if chans.is_empty() {
            continue;
        }
        let chans = if d.tx_collapsed { vec![] } else { chans };
        tx_cols.push((d, chans));
    }
    let mut rx_rows: Vec<(&DeviceEntry, Vec<&inferno_net::protocol::parser::RxChannel>)> =
        Vec::new();
    for d in &s.devices {
        let chans: Vec<_> = d.visible_rx(&rx_filter).collect();
        if chans.is_empty() {
            continue;
        }
        let chans = if d.rx_collapsed { vec![] } else { chans };
        rx_rows.push((d, chans));
    }

    let total_tx_cells: usize = tx_cols.iter().map(|(_, c)| c.len().max(1)).sum();

    rsx! {
        div { class: "min-w-full",
            // Filters row
            div { class: "flex items-center gap-3 px-4 py-2 border-b border-neutral-800 bg-neutral-900 sticky top-0 z-20",
                input {
                    class: "px-2 py-1 rounded bg-neutral-800 text-xs w-56 outline-none border border-neutral-700 focus:border-blue-500",
                    placeholder: "Filter Transmitters",
                    value: "{s.tx_filter}",
                    oninput: move |e| state.write().tx_filter = e.value(),
                }
                input {
                    class: "px-2 py-1 rounded bg-neutral-800 text-xs w-56 outline-none border border-neutral-700 focus:border-blue-500",
                    placeholder: "Filter Receivers",
                    value: "{s.rx_filter}",
                    oninput: move |e| state.write().rx_filter = e.value(),
                }
                span { class: "text-xs text-neutral-500 ml-auto",
                    "{tx_cols.len()} TX devices · {rx_rows.len()} RX devices"
                }
            }

            // Matrix grid
            MatrixGrid { state, total_tx_cells }
        }
    }
}

#[component]
fn MatrixGrid(state: Signal<AppState>, total_tx_cells: usize) -> Element {
    let cell = "w-6 h-6 min-w-6 min-h-6";
    let s = state.read();
    let tx_filter = s.tx_filter.clone();
    let rx_filter = s.rx_filter.clone();

    rsx! {
        div { class: "p-2",
            // TX header (devices then channels)
            div { class: "flex sticky top-12 z-10 bg-neutral-950 border-b border-neutral-800",
                // Spacer above the RX-label column
                div { class: "w-64 min-w-64 shrink-0" }
                for (di, d) in s.devices.iter().enumerate() {
                    {
                        let chans: Vec<_> = d.visible_tx(&tx_filter).collect();
                        if chans.is_empty() {
                            rsx! {}
                        } else if d.tx_collapsed {
                            rsx! {
                                TxDeviceHeader { state, device_index: di, span_cells: 1, collapsed: true }
                            }
                        } else {
                            rsx! {
                                TxDeviceHeader { state, device_index: di, span_cells: chans.len(), collapsed: false }
                            }
                        }
                    }
                }
            }
            // TX channel labels (rotated)
            div { class: "flex border-b border-neutral-800",
                div { class: "w-64 min-w-64 shrink-0" }
                for (_di, d) in s.devices.iter().enumerate() {
                    if !d.tx_collapsed {
                        for c in d.visible_tx(&tx_filter) {
                            div { class: "{cell} flex items-end justify-center",
                                title: "{c.name}@{d.name}",
                                span { class: "text-[10px] text-neutral-300 -rotate-90 whitespace-nowrap origin-bottom-left translate-y-3 translate-x-2",
                                    "{c.name}"
                                }
                            }
                        }
                    }
                }
            }

            // RX rows
            for (di, d) in s.devices.iter().enumerate() {
                {
                    let visible: Vec<_> = d.visible_rx(&rx_filter).collect();
                    if visible.is_empty() {
                        rsx! {}
                    } else {
                        rsx! {
                            // Device row label
                            RxDeviceRow { state, device_index: di, total_tx_cells }
                            // Per-channel rows
                            if !d.rx_collapsed {
                                for rc in visible {
                                    RxChannelRow {
                                        state,
                                        rx_device_index: di,
                                        rx_channel_number: rc.number,
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn TxDeviceHeader(
    state: Signal<AppState>,
    device_index: usize,
    span_cells: usize,
    collapsed: bool,
) -> Element {
    let width_px = (span_cells.max(1) * 24) as i32;
    let s = state.read();
    let d = &s.devices[device_index];
    let name = d.name.clone();
    drop(s);
    rsx! {
        div {
            class: "border-l border-neutral-800 px-2 py-1 text-[11px] font-medium truncate cursor-pointer hover:bg-neutral-800",
            style: "width: {width_px}px; min-width: {width_px}px;",
            onclick: move |_| {
                state.write().devices[device_index].tx_collapsed ^= true;
            },
            span { class: "text-neutral-400 mr-1", if collapsed { "▶" } else { "▼" } }
            "{name}"
        }
    }
}

#[component]
fn RxDeviceRow(state: Signal<AppState>, device_index: usize, total_tx_cells: usize) -> Element {
    let s = state.read();
    let d = &s.devices[device_index];
    let collapsed = d.rx_collapsed;
    let name = d.name.clone();
    let model = d.model.clone().unwrap_or_default();
    let ip = d.ip.to_string();
    drop(s);
    rsx! {
        div { class: "flex bg-neutral-900 border-y border-neutral-800",
            div {
                class: "w-64 min-w-64 shrink-0 px-2 py-1 text-xs font-medium cursor-pointer hover:bg-neutral-800 flex items-center gap-2",
                onclick: move |_| {
                    state.write().devices[device_index].rx_collapsed ^= true;
                },
                span { class: "text-neutral-400", if collapsed { "▶" } else { "▼" } }
                span { class: "truncate", "{name}" }
                span { class: "text-[10px] text-neutral-500 ml-auto", "{model}" }
                span { class: "text-[10px] text-neutral-600", "{ip}" }
            }
            // Reserve grid space; no per-cell content for the device-row.
            div { style: "width: {total_tx_cells * 24}px;" }
        }
    }
}

#[component]
fn RxChannelRow(
    state: Signal<AppState>,
    rx_device_index: usize,
    rx_channel_number: u16,
) -> Element {
    let s = state.read();
    let rx_dev = &s.devices[rx_device_index];
    let rx_filter = s.rx_filter.clone();
    let tx_filter = s.tx_filter.clone();
    let Some(rx_chan) = rx_dev
        .rx_channels
        .iter()
        .find(|c| c.number == rx_channel_number)
        .cloned()
    else {
        return rsx! {};
    };
    let rx_label = rx_chan.name.clone();
    let rx_dev_ip = rx_dev.ip;

    // Pre-collect TX columns to render alongside this row.
    let tx_columns: Vec<(
        String,
        inferno_net::protocol::parser::TxChannel,
        bool,
        usize,
    )> = s
        .devices
        .iter()
        .enumerate()
        .flat_map(|(_di, d)| {
            if d.tx_collapsed {
                return Vec::new();
            }
            let dev_name = d.name.clone();
            d.visible_tx(&tx_filter)
                .cloned()
                .map(|c| {
                    let active = matches!(
                        (&rx_chan.tx_device_name, &rx_chan.tx_channel_name),
                        (Some(td), Some(tc)) if td == &dev_name && tc == &c.name
                    );
                    (dev_name.clone(), c, active, _di)
                })
                .collect()
        })
        .collect();
    let _ = rx_filter;
    drop(s);

    rsx! {
        div { class: "flex border-b border-neutral-900 hover:bg-neutral-900/40",
            div { class: "w-64 min-w-64 shrink-0 pl-6 pr-2 py-0.5 text-xs flex items-center gap-2",
                span { class: "text-neutral-500 w-6 text-right", "{rx_channel_number}" }
                span { class: "truncate", "{rx_label}" }
            }
            for (tx_dev_name, tx_chan, active, _di) in tx_columns {
                MatrixCell {
                    state,
                    rx_device_ip: rx_dev_ip,
                    rx_device_index,
                    rx_channel_number,
                    tx_device_name: tx_dev_name,
                    tx_channel: tx_chan,
                    active,
                }
            }
        }
    }
}

#[component]
fn MatrixCell(
    state: Signal<AppState>,
    rx_device_ip: std::net::IpAddr,
    rx_device_index: usize,
    rx_channel_number: u16,
    tx_device_name: String,
    tx_channel: inferno_net::protocol::parser::TxChannel,
    active: bool,
) -> Element {
    let bg = if active {
        "bg-emerald-600 hover:bg-emerald-500"
    } else {
        "bg-neutral-900 hover:bg-neutral-700"
    };
    let glyph = if active { "✓" } else { "" };
    let title = format!(
        "{tx_chan}@{tx_dev}  →  channel {rx}",
        tx_chan = tx_channel.name,
        tx_dev = tx_device_name,
        rx = rx_channel_number,
    );
    rsx! {
        div {
            class: "w-6 h-6 min-w-6 min-h-6 border-l border-t border-neutral-800 grid place-items-center text-[11px] cursor-pointer {bg}",
            title: "{title}",
            onclick: move |_| {
                let tx_chan = tx_channel.clone();
                let tx_dev_name = tx_device_name.clone();
                spawn(async move {
                    let rx_chan_opt = state
                        .read()
                        .devices
                        .get(rx_device_index)
                        .and_then(|d| {
                            d.rx_channels
                                .iter()
                                .find(|c| c.number == rx_channel_number)
                                .cloned()
                        });
                    let Some(rx_chan) = rx_chan_opt else { return };
                    let status = toggle_subscription(rx_device_ip, rx_chan, &tx_dev_name, &tx_chan).await;
                    // Re-fetch the affected RX device so the matrix reflects new state.
                    let dev_name = state
                        .read()
                        .devices
                        .get(rx_device_index)
                        .map(|d| d.name.clone());
                    if let Some(name) = dev_name {
                        let updated = fetch_device(name, rx_device_ip).await;
                        let mut s = state.write();
                        if let Some(d) = s.devices.get_mut(rx_device_index) {
                            *d = updated;
                        }
                        s.status = Some(status);
                    }
                });
            },
            span { class: "text-white font-bold leading-none", "{glyph}" }
        }
    }
}
