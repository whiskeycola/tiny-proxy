use std::io;

use thiserror::Error;
use tokio::time::error::Elapsed;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io: {0}")]
    IO(#[from] io::Error),

    #[error("timeout: {0}")]
    Timeout(#[from] Elapsed),

    #[error("httparse: {0}")]
    HttParse(#[from] httparse::Error),

    #[error("read header: buffer overflow")]
    ReadHeaderBufferOverflow,

    #[error("read header: incorrect request")]
    ReadHeaderIncorrectRequest,

    #[error("proxy authentication ruquired")]
    AuthenticationRequired,

    #[error("not found header host")]
    NotFoundHeaderHost,

    #[error("connect: incorrect host")]
    IncorrectHost,

    #[error("dist connect")]
    DistConnect,

    #[error("unknown data store error")]
    Unknown,
}
