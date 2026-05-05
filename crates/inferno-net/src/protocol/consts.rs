//! Dante protocol constants. Mirrors `netaudio/dante/const.py`.

pub const DEVICE_ARC_PORT: u16 = 4440;
pub const DEVICE_CONTROL_PORT: u16 = 8800;
pub const DEVICE_INFO_PORT: u16 = 8702;
pub const DEVICE_SETTINGS_PORT: u16 = 8700;
pub const DEVICE_HEARTBEAT_PORT: u16 = 8708;
pub const DEVICE_LOCK_PORT: u16 = 8002;

pub const PROTOCOL_ID: u16 = 0x27FF;
pub const PROTOCOL_SETTINGS: u16 = 0xFFFF;
pub const PROTOCOL_CMC: u16 = 0x1200;
pub const PROTOCOL_AES67_CONFIG: u16 = 0x2809;

pub const CMC_COMMAND_REGISTER: u16 = 0x1003;
pub const CMC_OPCODE_METERING_START: u16 = 0x3010;

pub const RESULT_CODE_SUCCESS: u16 = 0x0001;
pub const RESULT_CODE_SUCCESS_EXTENDED: u16 = 0x8112;
pub const RESULT_CODE_LOCK_REJECTION: u16 = 0x0600;

pub const OPCODE_CHANNEL_COUNT: u16 = 0x1000;
pub const OPCODE_DEVICE_NAME_SET: u16 = 0x1001;
pub const OPCODE_DEVICE_NAME: u16 = 0x1002;
pub const OPCODE_DEVICE_INFO: u16 = 0x1003;
pub const OPCODE_DEVICE_SETTINGS: u16 = 0x1100;
pub const OPCODE_DEVICE_SETTINGS_SET: u16 = 0x1101;
pub const OPCODE_TX_CHANNEL_INFO: u16 = 0x2000;
pub const OPCODE_TX_CHANNEL_NAMES: u16 = 0x2010;
pub const OPCODE_TX_CHANNEL_NAME_SET: u16 = 0x2013;
pub const OPCODE_RX_CHANNELS: u16 = 0x3000;
pub const OPCODE_RX_CHANNEL_NAME_SET: u16 = 0x3001;
pub const OPCODE_SUBSCRIPTION_ADD: u16 = 0x3010;
pub const OPCODE_SUBSCRIPTION_REMOVE: u16 = 0x3014;

// Flow-control opcodes. Use the 10-byte flow header (see
// `DantePacket::build_flow_request`) and one of `FLOW_PROTOCOL_IDS` as
// the wire protocol. `_2809` is the AES67-config variant.
pub const OPCODE_QUERY_TX_FLOWS: u16 = 0x2200;
pub const OPCODE_CREATE_TX_FLOW: u16 = 0x2201;
pub const OPCODE_DELETE_TX_FLOW: u16 = 0x2202;
pub const OPCODE_QUERY_TX_FLOWS_2809: u16 = 0x2600;
pub const OPCODE_CREATE_TX_FLOW_2809: u16 = 0x2601;
pub const OPCODE_DELETE_TX_FLOW_2809: u16 = 0x2602;

/// Try in order to detect which flow protocol the device speaks.
pub const FLOW_PROTOCOL_IDS: &[u16] = &[0x2729, 0x2801, 0x2809];

pub const FLOW_TYPE_MULTICAST: u16 = 0x0002;

pub const DEVICE_SETTINGS_INFO_SAMPLE_RATE: u16 = 0x8020;
pub const DEVICE_SETTINGS_INFO_LATENCY: u16 = 0x8204;
pub const DEVICE_SETTINGS_INFO_LATENCY_MIN: u16 = 0x8205;
pub const DEVICE_SETTINGS_INFO_LATENCY_MAX: u16 = 0x8302;

pub const SUBSCRIPTION_STATUS_NONE: u16 = 0x0000;
