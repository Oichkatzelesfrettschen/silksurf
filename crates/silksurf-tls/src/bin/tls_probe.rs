use rustls::ClientConnection;
use silksurf_tls::{TlsConfig, root_store_diagnostics};
use std::net::TcpStream;
use std::time::Duration;

fn main() {
    let mut args = std::env::args().skip(1);
    let host = args.next().unwrap_or_else(|| "example.com".to_string());
    let port = args
        .next()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(443);

    let diagnostics = root_store_diagnostics();
    println!("TLS probe target: {host}:{port}");
    println!("Mozilla/webpki roots: {}", diagnostics.mozilla_roots);
    println!(
        "Native certs: loaded={}, added={}, rejected={}",
        diagnostics.native_certs_loaded,
        diagnostics.native_certs_added,
        diagnostics.native_certs_rejected
    );
    println!("Total rustls roots: {}", diagnostics.total_roots);
    println!(
        "Cert env: SSL_CERT_FILE={:?}, SSL_CERT_DIR={:?}, NIX_SSL_CERT_FILE={:?}",
        diagnostics.ssl_cert_file, diagnostics.ssl_cert_dir, diagnostics.nix_ssl_cert_file
    );

    if diagnostics.native_cert_errors.is_empty() {
        println!("Native cert loader errors: none");
    } else {
        println!("Native cert loader errors:");
        for error in &diagnostics.native_cert_errors {
            println!("  - {error}");
        }
    }

    match handshake(&host, port) {
        Ok(alpn) => {
            println!("TLS handshake: ok");
            println!("Negotiated ALPN: {alpn:?}");
        }
        Err(error) => {
            eprintln!("TLS handshake: failed: {error}");
            std::process::exit(1);
        }
    }
}

fn handshake(host: &str, port: u16) -> Result<Option<Vec<u8>>, String> {
    let addr = format!("{host}:{port}");
    let mut tcp = TcpStream::connect(&addr).map_err(|error| format!("TCP connect: {error}"))?;
    tcp.set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|error| format!("set read timeout: {error}"))?;
    tcp.set_write_timeout(Some(Duration::from_secs(30)))
        .map_err(|error| format!("set write timeout: {error}"))?;

    let server_name = rustls::pki_types::ServerName::try_from(host.to_string())
        .map_err(|error| format!("server name: {error}"))?;
    let mut conn = ClientConnection::new(TlsConfig::new().inner(), server_name)
        .map_err(|error| format!("client setup: {error}"))?;

    while conn.is_handshaking() {
        conn.complete_io(&mut tcp)
            .map_err(|error| format!("rustls complete_io: {error}"))?;
    }

    Ok(conn.alpn_protocol().map(Vec::from))
}
