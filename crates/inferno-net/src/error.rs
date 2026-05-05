use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("mdns error: {0}")]
    Mdns(String),

    #[error("protocol error: {0}")]
    Protocol(String),

    #[error("device not found: {0}")]
    DeviceNotFound(String),

    #[error("timeout")]
    Timeout,
}
