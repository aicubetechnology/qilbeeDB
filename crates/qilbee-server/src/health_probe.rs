//! Bounded, loopback-only container health probe without a separate HTTP client package.
use std::{
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream},
    time::Duration,
};

const MAX_RESPONSE_BYTES: usize = 8192;

pub fn check() -> io::Result<()> {
    probe_at(SocketAddr::from(([127, 0, 0, 1], 7474)))
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Unexpected QilbeeDB health response",
    )
}

fn validate_response(bytes: &[u8]) -> io::Result<()> {
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(invalid());
    }
    let boundary = bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(invalid)?;
    let headers = std::str::from_utf8(&bytes[..boundary]).map_err(|_| invalid())?;
    let status = headers.lines().next().ok_or_else(invalid)?;
    if !matches!(status, "HTTP/1.1 200 OK" | "HTTP/1.0 200 OK") {
        return Err(invalid());
    }
    let body: serde_json::Value =
        serde_json::from_slice(&bytes[boundary + 4..]).map_err(|_| invalid())?;
    if body["contract_version"] != 1
        || body["status"] != "healthy"
        || body["version"] != env!("CARGO_PKG_VERSION")
    {
        return Err(invalid());
    }
    Ok(())
}

fn probe_at(address: SocketAddr) -> io::Result<()> {
    let timeout = Duration::from_secs(2);
    let mut stream = TcpStream::connect_timeout(&address, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
    let mut bytes = Vec::new();
    stream
        .take(MAX_RESPONSE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    validate_response(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn response(status: &str, version: &str) -> Vec<u8> {
        format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{{\"contract_version\":1,\"status\":\"{status}\",\"version\":\"{version}\"}}").into_bytes()
    }

    #[test]
    fn rejects_error_redirect_wrong_version_and_oversized_health_responses() {
        let valid = response("healthy", env!("CARGO_PKG_VERSION"));
        assert!(validate_response(&valid).is_ok());
        assert!(validate_response(&response("unhealthy", env!("CARGO_PKG_VERSION"))).is_err());
        assert!(validate_response(&response("healthy", "unknown")).is_err());
        assert!(
            validate_response(b"HTTP/1.1 302 Found\r\nLocation: https://other.invalid\r\n\r\n")
                .is_err()
        );
        assert!(validate_response(b"HTTP/1.1 500 Internal Server Error\r\n\r\n{}").is_err());
        assert!(validate_response(&vec![b'x'; MAX_RESPONSE_BYTES + 1]).is_err());
        assert!(validate_response(b"HTTP/1.1 200 OK\r\n\r\nnot-json").is_err());
    }

    #[test]
    fn actual_tcp_probe_requests_only_health_and_accepts_the_current_version() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut connection, _) = listener.accept().unwrap();
            connection
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = [0u8; 128];
            let n = connection.read(&mut request).unwrap();
            assert_eq!(
                &request[..n],
                b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
            );
            connection
                .write_all(&response("healthy", env!("CARGO_PKG_VERSION")))
                .unwrap();
        });
        assert!(probe_at(address).is_ok());
        server.join().unwrap();
    }
}
