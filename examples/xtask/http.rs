use anyhow::{Context, ensure};
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::{HeaderMap, Request};
use hyper_util::rt::TokioIo;
use std::{net::SocketAddr, time::Duration};

pub(super) struct Response {
    pub status: u16,
    pub body: Bytes,
    pub headers: HeaderMap,
}
struct Connection(tokio::task::JoinHandle<()>);
impl Drop for Connection {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub(super) async fn request(
    address: SocketAddr,
    method: &str,
    path: &str,
) -> anyhow::Result<Response> {
    ensure!(
        address.ip().is_loopback(),
        "smoke requests must remain on loopback"
    );
    tokio::time::timeout(Duration::from_secs(3), async {
        let stream = tokio::net::TcpStream::connect(address).await?;
        let (mut sender, connection) =
            hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
        let _connection = Connection(tokio::spawn(async move {
            let _ = connection.await;
        }));
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("host", address.to_string())
            .header("connection", "close")
            .body(Full::new(Bytes::new()))?;
        let response = sender.send_request(request).await?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = Limited::new(response.into_body(), 1024 * 1024)
            .collect()
            .await
            .map_err(|error| anyhow::anyhow!("read limited response body: {error}"))?
            .to_bytes();
        Ok::<_, anyhow::Error>(Response {
            status,
            headers,
            body,
        })
    })
    .await
    .context("HTTP probe timed out")?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    #[tokio::test]
    async fn chunked_bodies_and_case_insensitive_headers_are_supported() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut input = Vec::new();
            let mut byte = [0];
            while !input.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).unwrap();
                input.push(byte[0]);
            }
            socket.write_all(b"HTTP/1.1 503 Unavailable\r\ncOnTeNt-TyPe: text/plain\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n2\r\nok\r\n0\r\n\r\n").unwrap();
        });
        let response = request(address, "GET", "/ready").await.unwrap();
        server.join().unwrap();
        assert_eq!(response.status, 503);
        assert_eq!(response.body.as_ref(), b"ok");
        assert_eq!(response.headers["content-type"], "text/plain");
    }
    #[tokio::test]
    async fn non_loopback_addresses_are_rejected_before_connection() {
        assert!(
            request("192.0.2.1:9100".parse().unwrap(), "GET", "/health")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn oversized_responses_are_rejected() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut input = Vec::new();
            let mut byte = [0];
            while !input.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).unwrap();
                input.push(byte[0]);
            }
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 1048577\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
            let _ = socket.write_all(&vec![b'x'; 1024 * 1024 + 1]);
        });
        let error = request(address, "GET", "/health").await.err().unwrap();
        server.join().unwrap();
        assert!(
            format!("{error:#}").contains("length limit exceeded"),
            "{error:#}"
        );
    }
}
