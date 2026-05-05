//! Dante control protocol — binary codec + async UDP client.
//!
//! Ported from the Python reference implementation
//! `network-audio-controller` (`netaudio/dante/protocol.py`,
//! `device_commands.py`, `device_parser.py`).

pub mod client;
pub mod commands;
pub mod consts;
pub mod packet;
pub mod parser;

pub use client::DanteClient;
pub use consts::*;
pub use packet::{DantePacket, DanteResponse};
pub use parser::{ChannelCount, DeviceInfo, DeviceSettings, RxChannel, TxChannel};
