use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
    time::Duration,
};

use httparse::Header;
use tokio::{
    io::{copy_bidirectional, AsyncRead, AsyncWrite, AsyncWriteExt},
    net::TcpSocket,
};
use tokio::{net::lookup_host, time::timeout};
pub mod error;
pub mod util;
pub use error::{Error, Result};
use util::{auth, read_header, write_200_established, write_400, write_500};
#[derive(Clone, Debug)]
pub struct User {
    pub login: String,
    pub pass: String,
}

#[derive(Default, Clone, Debug)]
pub struct Proxy {
    pub users: Vec<User>,
    pub bind_ipv4: Option<Ipv4Addr>,
    pub bind_ipv6: Option<Ipv6Addr>,
    pub nodelay: bool,
    pub connect_timeout: Option<Duration>,
}

impl Proxy {
    pub fn add_user(mut self, user: impl Into<String>, pass: impl Into<String>) -> Self {
        self.users.push(User {
            login: user.into(),
            pass: pass.into(),
        });
        self
    }

    pub fn set_bind_ipv4(mut self, addr: Ipv4Addr) -> Self {
        self.bind_ipv4 = Some(addr);
        self
    }

    pub fn set_bind_ipv6(mut self, addr: Ipv6Addr) -> Self {
        self.bind_ipv6 = Some(addr);
        self
    }
    pub fn set_nodelay(mut self, nodelay: bool) -> Self {
        self.nodelay = nodelay;
        self
    }

    pub async fn run<RW: AsyncRead + AsyncWrite + std::marker::Unpin>(
        &self,
        mut stream: RW,
    ) -> Result<()> {
        let mut buf = [0u8; 32 * 1024];

        // read header
        let (headers_len, buf_len) = match read_header(&mut stream, &mut buf).await {
            Ok(r) => r,
            Err(e) => {
                if matches!(
                    e,
                    Error::ReadHeaderIncorrectRequest | Error::ReadHeaderBufferOverflow
                ) {
                    write_400(stream).await?;
                }
                return Err(e);
            }
        };

        let mut headers = [httparse::EMPTY_HEADER; 32];
        let mut req = httparse::Request::new(&mut headers);
        if req.parse(&buf[..headers_len])?.is_partial() {
            write_400(stream).await?;
            return Err(Error::ReadHeaderIncorrectRequest);
        }

        let (Some(method), Some(path), Some(version)) = (req.method, req.path, req.version) else {
            write_400(stream).await?;
            return Err(Error::ReadHeaderIncorrectRequest);
        };

        // auth
        if !self.users.is_empty() {
            let proxy_authorization = req
                .headers
                .iter()
                // .filter(|h| !h.name.is_empty())
                .find(|h| h.name.to_lowercase() == "proxy-authorization")
                .map(|h| h.value)
                .map(String::from_utf8_lossy)
                .and_then(|a| auth(&self.users, a.as_ref()));

            if !matches!(proxy_authorization, Some(true)) {
                stream.write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic\r\n\r\n").await?;
                stream.flush().await?;
                return Err(Error::AuthenticationRequired);
            }
        }

        let mut dest_stream = if method.to_uppercase() == "CONNECT" {
            let dest_stream = match self.connect_with_timeout(path, "443").await {
                Ok(d) => d,
                Err(e) => {
                    write_500(stream).await?;
                    return Err(e);
                }
            };
            write_200_established(&mut stream).await?;
            dest_stream
        } else {
            let Some(host) = req
                .headers
                .iter()
                .find(|h| h.name.to_lowercase() == "host")
                .map(|h| h.value)
                .map(|v| String::from_utf8_lossy(v).to_string())
            else {
                write_400(stream).await?;
                return Err(Error::NotFoundHeaderHost);
            };

            let mut dest_stream = match self.connect_with_timeout(host.as_ref(), "80").await {
                Ok(d) => d,
                Err(e) => {
                    write_500(stream).await?;
                    return Err(e);
                }
            };

            self.write_http(
                &mut dest_stream,
                method,
                path,
                version,
                req.headers,
                &buf[headers_len..buf_len],
            )
            .await?;

            dest_stream
        };

        copy_bidirectional(&mut stream, &mut dest_stream).await?;

        Ok(())
    }

    async fn connect_with_timeout(
        &self,
        host: &str,
        default_port: &str,
    ) -> Result<impl AsyncRead + AsyncWrite + std::marker::Unpin> {
        match self.connect_timeout {
            Some(duration) => timeout(duration, self.connect(host, default_port)).await?,
            None => self.connect(host, default_port).await,
        }
    }

    async fn connect(
        &self,
        host: &str,
        default_port: &str,
    ) -> Result<impl AsyncRead + AsyncWrite + std::marker::Unpin> {
        let mut splited = host.splitn(2, ":");
        // TODO mybe need urldecode
        let host = splited.next().ok_or(Error::IncorrectHost)?;
        let port = splited.next().unwrap_or(default_port);

        let mut addr_ipv4 = Vec::new();
        let mut addr_ipv6 = Vec::new();

        for addr in lookup_host(format!("{host}:{port}")).await? {
            if addr.is_ipv4() {
                addr_ipv4.push(addr)
            } else {
                addr_ipv6.push(addr)
            }
        }

        if let Some(ipv6) = self.bind_ipv6 {
            for con_addr in addr_ipv6 {
                let saddr = SocketAddrV6::new(ipv6, 0, 0, 0);
                let socket = TcpSocket::new_v6()?;
                socket.bind(SocketAddr::V6(saddr))?;

                if let Ok(stream) = socket.connect(con_addr).await {
                    return Ok(stream);
                }
            }
        }

        if let Some(ipv4) = self.bind_ipv4 {
            for con_addr in addr_ipv4.clone() {
                let saddr = SocketAddrV4::new(ipv4, 0);
                let socket = TcpSocket::new_v4()?;
                socket.bind(SocketAddr::V4(saddr))?;

                if let Ok(stream) = socket.connect(con_addr).await {
                    return Ok(stream);
                }
            }
        }

        for con_addr in addr_ipv4.clone() {
            let socket = TcpSocket::new_v4()?;
            if let Ok(stream) = socket.connect(con_addr).await {
                return Ok(stream);
            }
        }
        Err(Error::DistConnect)
    }

    async fn write_http<'h, 'b, W: AsyncWrite + Unpin>(
        &self,
        stream: &mut W,
        method: &'b str,
        path: &'b str,
        version: u8,
        headers: &'h [Header<'b>],
        body: &'b [u8],
    ) -> Result<()> {
        stream
            .write_all(format!("{method} {path} HTTP/1.{version}\r\n").as_bytes())
            .await?;

        for h in headers {
            if h.name.to_lowercase() == "proxy-authorization" {
                continue;
            }

            stream.write_all(h.name.as_bytes()).await?;
            stream.write_all(b": ").await?;
            stream.write_all(h.value).await?;
            stream.write_all(b"\r\n").await?;
        }

        stream.write_all(b"\r\n").await?;

        if !body.is_empty() {
            stream.write_all(body).await?;
        }

        stream.flush().await?;
        Ok(())
    }
}
