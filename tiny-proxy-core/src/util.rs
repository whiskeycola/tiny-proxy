use crate::{Error, Result, User};
use base64::{prelude::BASE64_STANDARD, Engine as _};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub(crate) async fn write_400(mut stream: impl AsyncWrite + Unpin) -> Result<()> {
    stream
        .write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n")
        .await?;
    stream.flush().await?;
    Ok(())
}

pub(crate) async fn write_500(mut stream: impl AsyncWrite + Unpin) -> Result<()> {
    stream.write_all(b"HTTP/1.1 500 Internal\r\n\r\n").await?;
    stream.flush().await?;
    Ok(())
}
pub(crate) async fn write_200_established<W: AsyncWrite + Unpin>(stream: &mut W) -> Result<()> {
    stream
        .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
        .await?;
    stream.flush().await?;
    Ok(())
}

pub(crate) fn auth(users: &[User], header: &str) -> Option<bool> {
    let (auth_type, auth_data) = header.trim().split_once(" ")?;
    // let splited = header.splitn(2, "")
    if auth_type != "Basic" {
        return None;
    }
    let auth_data = BASE64_STANDARD.decode(auth_data).ok()?;
    let auth_data = String::from_utf8(auth_data).ok()?;
    // let (login, pass) = auth_data.split_once(":")?;
    let mut splited = auth_data.splitn(2, ":");
    let login = splited.next()?;
    let pass = splited.next().unwrap_or_default();
    let result = users.iter().any(|u| u.login == login && u.pass == pass);

    Some(result)
}

pub(crate) async fn read_header<R: AsyncRead + std::marker::Unpin>(
    stream: &mut R,
    buf: &mut [u8],
) -> Result<(usize, usize)> {
    let mut buf_len = 0;
    loop {
        if buf_len >= buf.len() {
            return Err(Error::ReadHeaderBufferOverflow);
        }
        let n = stream.read(&mut buf[buf_len..]).await?;
        if n == 0 {
            return Err(Error::ReadHeaderIncorrectRequest);
        }
        let mut idx = buf_len;
        buf_len += n;

        // safe get_unchecked
        if idx < 2 {
            idx = 2;
        }
        while idx < buf_len {
            if *unsafe { buf.get_unchecked(idx) } == b'\n' {
                match unsafe { buf.get_unchecked(idx - 1) } {
                    b'\n' => {
                        return Ok((idx + 1, buf_len));
                    }
                    b'\r' => {
                        if *unsafe { buf.get_unchecked(idx - 2) } == b'\n' {
                            return Ok((idx + 1, buf_len));
                        }
                    }
                    _ => {}
                }
            }
            idx += 1;
        }
    }
}
#[cfg(test)]
mod test {
    use crate::util::read_header;
    use crate::Error;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn test_reader_header_one_byte_err() {
        const H: &[u8] = b"GET /hello world";
        let mut buf = [0; H.len()];
        let mut data = BufReader::new(H);
        let res = read_header(&mut data, &mut buf).await.unwrap_err();
        assert!(matches!(res, Error::ReadHeaderBufferOverflow))
    }
    #[tokio::test]
    async fn test_reader_header_one_line_err() {
        const H: &[u8] = b"GET /hello world";
        let mut buf = [0; H.len()];
        let mut data = BufReader::new(H);
        let res = read_header(&mut data, &mut buf).await.unwrap_err();
        assert!(matches!(res, Error::ReadHeaderBufferOverflow))
    }

    #[tokio::test]
    async fn test_reader_header_ok() {
        const H: &[u8] = b"GET /hello world\r\nHost: hello-world.com\r\n\r\n";
        let mut buf = [0; H.len()];
        let mut data = BufReader::new(H);
        let res = read_header(&mut data, &mut buf).await.unwrap();
        assert_eq!(res, (43, 43));
    }
    #[tokio::test]
    async fn test_reader_header_data_ok() {
        const H: &[u8] = b"GET /hello world\r\nHost: hello-world.com\r\n\r\ndata";
        let mut buf = [0; H.len()];
        let mut data = BufReader::new(H);
        let (idx, _) = read_header(&mut data, &mut buf).await.unwrap();
        assert_eq!(idx, 43);
    }
    #[tokio::test]
    async fn test_reader_header_errror_incorrect_data() {
        const H: &[u8] = b"";
        let mut buf = [0; 50];
        let mut data = BufReader::new(H);
        let res = read_header(&mut data, &mut buf).await.unwrap_err();
        assert!(
            matches!(res, Error::ReadHeaderIncorrectRequest),
            "expected error ReadHeaderIncorrectData"
        );
    }
    #[tokio::test]
    async fn test_reader_header_error_buffer_overflow() {
        const H: &[u8] = b"GET /hello world\r\nHost: hello-world.com\r\n\rdata";
        let mut buf = [0; H.len()];
        let mut data = BufReader::new(H);
        let res = read_header(&mut data, &mut buf).await.unwrap_err();
        assert!(
            matches!(res, Error::ReadHeaderBufferOverflow),
            "expected error ReadHeaderBufferOverflow"
        );
    }
}
