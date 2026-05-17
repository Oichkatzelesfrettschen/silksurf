/*
 * tls_probe -- TLS + DANE diagnostic tool for SilkSurf.
 *
 * WHY: SilkSurf fetches HTTPS resources using rustls with Mozilla and native
 * root certificates. When a connection fails with UnknownIssuer or similar
 * errors, the operator needs to know: which CA signed the server cert, whether
 * the chain is valid from available roots, and optionally whether a DANE TLSA
 * record in DNS would have provided an alternative trust path.
 *
 * This tool reports three independent diagnostics in one pass:
 *
 *  1. Root store inventory    -- Mozilla roots, native certs, env vars
 *  2. TLS handshake detail    -- cipher suite, protocol version, ALPN result
 *  3. Certificate chain       -- subject, issuer, validity window, SANs, SHA-256
 *  4. DANE TLSA probe         -- lookup _<port>._tcp.<host> TLSA records:
 *                                 no-tlsa       no record published
 *                                 records-found TLSA data and type reported
 *                                 bogus         DNSSEC validation failed
 *
 * On handshake failure the tool prints an RCA (root cause analysis) with
 * actionable remediation steps tailored to the specific error and the state
 * of the local root store.
 *
 * Usage: tls-probe <host> [port] [--ca <pem>]
 *        tls-probe example.com
 *        tls-probe example.com 8443
 *        tls-probe example.com --ca /etc/ssl/corp.pem
 *
 * See: silksurf-tls/src/lib.rs RootStoreDiagnostics, TlsConfig
 * See: silksurf-app/src/main.rs  --tls-ca-file flag integration
 */

use hickory_proto::rr::RData;
use hickory_resolver::Resolver;
use rustls::pki_types::ServerName;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 || args[1] == "--help" || args[1] == "-h" {
        eprintln!("Usage: tls-probe <host> [port] [--ca <pem-file>]");
        eprintln!("       tls-probe example.com");
        eprintln!("       tls-probe example.com 8443");
        eprintln!("       tls-probe example.com --ca /etc/ssl/corp.pem");
        std::process::exit(1);
    }

    let host = args[1].clone();
    let port: u16 = args
        .iter()
        .skip(2)
        .find(|a| !a.starts_with('-'))
        .and_then(|s| s.parse().ok())
        .unwrap_or(443);

    let ca_file: Option<PathBuf> = args
        .windows(2)
        .find_map(|w| {
            if w[0] == "--ca" {
                Some(PathBuf::from(&w[1]))
            } else {
                None
            }
        })
        .or_else(|| {
            args.iter()
                .find_map(|a| a.strip_prefix("--ca=").map(PathBuf::from))
        });

    println!("=== tls-probe: {host}:{port} ===");

    // -------------------------------------------------------------------------
    // 1. Root store diagnostics
    // -------------------------------------------------------------------------
    let diag = silksurf_tls::root_store_diagnostics();
    println!("\n[root store]");
    println!("  mozilla roots  : {}", diag.mozilla_roots);
    println!("  native loaded  : {}", diag.native_certs_loaded);
    println!("  native added   : {}", diag.native_certs_added);
    println!("  native rejected: {}", diag.native_certs_rejected);
    println!("  total roots    : {}", diag.total_roots);
    if let Some(ref f) = diag.ssl_cert_file {
        println!("  SSL_CERT_FILE  : {f}");
    }
    if let Some(ref d) = diag.ssl_cert_dir {
        println!("  SSL_CERT_DIR   : {d}");
    }
    if let Some(ref f) = diag.nix_ssl_cert_file {
        println!("  NIX_SSL_CERT_FILE: {f}");
    }
    for e in &diag.native_cert_errors {
        println!("  native error   : {e}");
    }

    // -------------------------------------------------------------------------
    // 2. Build TLS client config
    // -------------------------------------------------------------------------
    let tls_config = if let Some(ref path) = ca_file {
        println!("\n[tls] using extra CA bundle: {}", path.display());
        match silksurf_tls::TlsConfig::new_with_extra_ca_file(path) {
            Ok(cfg) => cfg,
            Err(e) => {
                println!("[tls] ERROR loading CA file: {e}");
                std::process::exit(2);
            }
        }
    } else {
        silksurf_tls::TlsConfig::new()
    };

    // -------------------------------------------------------------------------
    // 3. TLS handshake
    // -------------------------------------------------------------------------
    println!("\n[tls] connecting to {host}:{port}...");
    let addr = format!("{host}:{port}");
    let tcp = match TcpStream::connect(&addr) {
        Ok(s) => s,
        Err(e) => {
            println!("[tls] TCP connect error: {e}");
            std::process::exit(3);
        }
    };

    let server_name = match ServerName::try_from(host.as_str()).map(|n| n.to_owned()) {
        Ok(n) => n,
        Err(e) => {
            println!("[tls] invalid server name: {e}");
            std::process::exit(3);
        }
    };

    let mut conn = match rustls::ClientConnection::new(tls_config.inner(), server_name) {
        Ok(c) => c,
        Err(e) => {
            println!("[tls] ClientConnection::new error: {e}");
            std::process::exit(3);
        }
    };

    let mut tcp = tcp;
    let handshake_result = (|| {
        while conn.is_handshaking() {
            conn.complete_io(&mut tcp)?;
        }
        Ok::<(), std::io::Error>(())
    })();

    match handshake_result {
        Ok(()) => println!("[tls] handshake OK"),
        Err(e) => {
            /*
             * Alert/handshake errors surface as io::Error wrapping rustls::Error.
             * Print the full debug representation to get the exact alert code.
             */
            println!("[tls] handshake FAILED: {e:?}");
            println!("[tls] (partial) protocol: {:?}", conn.protocol_version());
            println!(
                "[tls] (partial) cipher: {:?}",
                conn.negotiated_cipher_suite()
            );
            print_peer_certs(&conn);

            println!("\n--- root cause analysis ---");
            diagnose_tls_error(&e, &host, &diag);

            println!("\n[dane] running TLSA probe regardless...");
            probe_dane(&host, port);
            std::process::exit(4);
        }
    }

    // -------------------------------------------------------------------------
    // 4. Report handshake details
    // -------------------------------------------------------------------------
    println!("[tls] protocol  : {:?}", conn.protocol_version());
    println!("[tls] cipher    : {:?}", conn.negotiated_cipher_suite());
    println!("[tls] alpn      : {:?}", conn.alpn_protocol());

    print_peer_certs(&conn);

    // Send a minimal HEAD request so the server flushes its response.
    {
        let mut stream = rustls::StreamOwned::new(conn, tcp);
        let req = format!(
            "HEAD / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nUser-Agent: SilkSurf-tls-probe/0.1\r\n\r\n"
        );
        let _ = stream.write_all(req.as_bytes());
        let mut resp_buf = [0u8; 512];
        let _ = stream.read(&mut resp_buf);
        let resp_str = std::str::from_utf8(&resp_buf).unwrap_or("(non-utf8)");
        let status_line = resp_str.lines().next().unwrap_or("(empty)");
        println!("[http] status   : {status_line}");
    }

    // -------------------------------------------------------------------------
    // 5. DANE TLSA probe
    // -------------------------------------------------------------------------
    println!();
    probe_dane(&host, port);

    println!("\n=== probe complete ===");
}

// =============================================================================
// Certificate chain display
// =============================================================================

/*
 * print_peer_certs -- decode and display the server certificate chain.
 *
 * WHY: The most common TLS failure is a certificate signed by a CA not in
 * our trust store. Printing the issuer of each cert in the chain shows
 * exactly which root or intermediate is absent.
 *
 * The cert subject/issuer are decoded via a minimal ASN.1 DER parser in the
 * asn1_cert module below. No additional crate dependency required.
 *
 * Complexity: O(chain_len * cert_size), typically 2-4 certs at <4KB each.
 */
fn print_peer_certs(conn: &rustls::ClientConnection) {
    let Some(certs) = conn.peer_certificates() else {
        println!("[cert] no peer certificates available");
        return;
    };

    println!("\n[cert chain] {} certificate(s)", certs.len());
    for (i, cert_der) in certs.iter().enumerate() {
        println!("  cert[{i}]:");
        println!("    DER size  : {} bytes", cert_der.len());
        match asn1_cert::parse_cert_info(cert_der.as_ref()) {
            Some(info) => {
                println!("    subject   : {}", info.subject);
                println!("    issuer    : {}", info.issuer);
                println!("    not_before: {}", info.not_before);
                println!("    not_after : {}", info.not_after);
                if !info.sans.is_empty() {
                    println!("    SANs      : {}", info.sans.join(", "));
                }
                let fp = sha256_hex(cert_der.as_ref());
                println!("    sha256    : {fp}");
            }
            None => {
                println!(
                    "    (could not decode; DER starts with {:02x?})",
                    &cert_der.as_ref()[..cert_der.as_ref().len().min(8)]
                );
            }
        }
    }
}

// =============================================================================
// DANE TLSA probe
// =============================================================================

/*
 * probe_dane -- perform TLSA DNS lookup for _<port>._tcp.<host>.
 *
 * WHY: DANE (RFC 6698) lets domain operators publish certificate fingerprints
 * or CA constraints in DNS (secured by DNSSEC). This probe shows what is
 * published so the operator can decide whether DANE would resolve a WebPKI
 * failure or whether the published data matches the served certificate.
 *
 * The hickory-resolver with dnssec-ring feature is used so DNSSEC validation
 * errors surface as resolver errors (SERVFAIL) rather than being silently
 * accepted. The resolver is driven via a tokio current_thread runtime.
 *
 * States reported:
 *   no-tlsa       NXDOMAIN or empty answer -- no TLSA record published
 *   records-found TLSA data present; type/data printed for manual comparison
 *   bogus         DNSSEC validation failure -- record cannot be trusted
 *   error         Resolver or network error unrelated to DNSSEC
 */
fn probe_dane(host: &str, port: u16) {
    // Trailing dot forces a FQDN lookup, preventing the resolver from appending
    // the search domain from /etc/resolv.conf (e.g. ".localdomain").
    let tlsa_name = format!("_{port}._tcp.{host}.");
    println!("[dane] querying TLSA for {tlsa_name} ...");

    // Build a tokio single-thread runtime just for the async resolver.
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            println!("[dane] runtime error: {e}");
            return;
        }
    };

    let result = rt.block_on(async {
        let mut builder = match Resolver::builder_tokio() {
            Ok(b) => b,
            Err(e) => return Err(format!("resolver builder: {e}")),
        };
        // Request DNSSEC validation from the upstream resolver.
        builder.options_mut().validate = true;
        let resolver = match builder.build() {
            Ok(r) => r,
            Err(e) => return Err(format!("resolver build: {e}")),
        };

        resolver
            .tlsa_lookup(tlsa_name.as_str())
            .await
            .map_err(|e| format!("{e}"))
    });

    match result {
        Ok(lookup) => {
            let records: Vec<&hickory_proto::rr::rdata::TLSA> = lookup
                .answers()
                .iter()
                .filter_map(|rec| match &rec.data {
                    RData::TLSA(tlsa) => Some(tlsa),
                    _ => None,
                })
                .collect();

            if records.is_empty() {
                println!("[dane] no-tlsa (empty answer for {tlsa_name})");
                return;
            }

            println!("[dane] {} TLSA record(s):", records.len());
            for (i, tlsa) in records.iter().enumerate() {
                let usage_str = match tlsa.cert_usage {
                    hickory_proto::rr::rdata::tlsa::CertUsage::PkixTa => {
                        "0 PKIX-TA (CA constraint)"
                    }
                    hickory_proto::rr::rdata::tlsa::CertUsage::PkixEe => {
                        "1 PKIX-EE (service cert constraint)"
                    }
                    hickory_proto::rr::rdata::tlsa::CertUsage::DaneTa => {
                        "2 DANE-TA (trust anchor assertion)"
                    }
                    hickory_proto::rr::rdata::tlsa::CertUsage::DaneEe => {
                        "3 DANE-EE (domain-issued cert)"
                    }
                    hickory_proto::rr::rdata::tlsa::CertUsage::Unassigned(v) => {
                        &format!("? (unassigned {v})")
                    }
                    hickory_proto::rr::rdata::tlsa::CertUsage::Private => "255 (private)",
                };
                let selector_str = match tlsa.selector {
                    hickory_proto::rr::rdata::tlsa::Selector::Full => "0 (full cert)",
                    hickory_proto::rr::rdata::tlsa::Selector::Spki => "1 (SubjectPublicKeyInfo)",
                    hickory_proto::rr::rdata::tlsa::Selector::Unassigned(v) => {
                        &format!("? (unassigned {v})")
                    }
                    hickory_proto::rr::rdata::tlsa::Selector::Private => "255 (private)",
                };
                let matching_str = match tlsa.matching {
                    hickory_proto::rr::rdata::tlsa::Matching::Raw => {
                        "0 (full -- not hash-compared)"
                    }
                    hickory_proto::rr::rdata::tlsa::Matching::Sha256 => "1 SHA-256",
                    hickory_proto::rr::rdata::tlsa::Matching::Sha512 => "2 SHA-512",
                    hickory_proto::rr::rdata::tlsa::Matching::Unassigned(v) => {
                        &format!("? (unassigned {v})")
                    }
                    hickory_proto::rr::rdata::tlsa::Matching::Private => "255 (private)",
                };
                let data_hex: String = tlsa.cert_data.iter().map(|b| format!("{b:02x}")).collect();
                println!(
                    "  record[{i}]  usage={usage_str}  selector={selector_str}  matching={matching_str}"
                );
                println!("             data={data_hex}");
            }
            println!(
                "[dane] result: records-found (compare sha256 above with SHA-256 records for match check)"
            );
        }
        Err(msg) => {
            if msg.contains("SERVFAIL") || msg.contains("DNSSEC") || msg.contains("Bogus") {
                println!("[dane] bogus -- DNSSEC validation failed: {msg}");
            } else if msg.contains("NXDOMAIN")
                || msg.contains("NoRecordsFound")
                || msg.contains("no record")
            {
                println!("[dane] no-tlsa -- NXDOMAIN/empty: {msg}");
            } else {
                println!("[dane] error: {msg}");
            }
        }
    }
}

// =============================================================================
// Root cause analysis
// =============================================================================

/*
 * diagnose_tls_error -- human-readable RCA for a failed TLS handshake.
 *
 * WHY: rustls error messages are correct but terse. This function maps the
 * common error patterns to actionable remediation, using the root store
 * diagnostics to give count-sensitive advice (e.g. 0 native certs loaded
 * strongly suggests a Nix/container environment missing SSL_CERT_FILE).
 *
 * Error patterns and remediation:
 *
 *   UnknownIssuer / unknown ca
 *     -- CA not in trust store.  Two distinct root causes:
 *     (a) Nix/container: native_certs_loaded == 0, set SSL_CERT_FILE.
 *     (b) Incomplete chain: server omitted an intermediate; the issuing CA
 *         is simply absent from the handshake.  curl -v shows the same error
 *         (OpenSSL error 20: UNABLE_TO_GET_ISSUER_CERT_LOCALLY).  Fix:
 *         fetch the intermediate via AIA or supply with --tls-ca-file.
 *     (c) Corporate proxy / private PKI: supply root CA with --tls-ca-file.
 *
 *   InvalidCertificate Expired
 *     -- Server cert has expired; renew or use --insecure as last resort.
 *
 *   InvalidCertificate NotValidYet
 *     -- Clock skew; check ntpd/chrony.
 *
 *   InvalidCertificate NotValidForName
 *     -- SNI mismatch; wrong hostname or IP.
 *
 *   PeerSentFatalAlert HandshakeFailure
 *     -- Cipher/protocol mismatch with server.
 *
 *   NoCertificatesPresented
 *     -- Not a TLS service on this port.
 */
fn diagnose_tls_error(e: &std::io::Error, host: &str, diag: &silksurf_tls::RootStoreDiagnostics) {
    let msg = format!("{e:?}");

    if msg.contains("UnknownIssuer") || msg.contains("unknown ca") {
        println!("  CAUSE: Server certificate signed by a CA not in our trust store.");
        if diag.native_certs_loaded == 0 {
            println!(
                "  ROOT : 0 native certs loaded -- likely Nix shell / container with no SSL_CERT_FILE."
            );
            println!("  FIX A: export SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt");
            println!(
                "  FIX B: export NIX_SSL_CERT_FILE=$(nix eval --raw nixpkgs#cacert)/etc/ssl/certs/ca-bundle.crt"
            );
            println!("  FIX C: tls-probe {host} --ca /path/to/ca-bundle.pem");
            println!(
                "  FIX D: silksurf-app --tls-ca-file /path/to/ca-bundle.pem https://{host}"
            );
        } else {
            println!(
                "  ROOT : {} native certs loaded but issuing CA absent -- corporate proxy or private PKI.",
                diag.native_certs_loaded
            );
            println!(
                "  NOTE : Also possible: server sent an incomplete chain (missing intermediate)."
            );
            println!(
                "         Verify with: curl -v https://{host} 2>&1 | grep -E 'SSL|certificate'"
            );
            println!(
                "         OpenSSL error 20 (unable to get local issuer) confirms incomplete chain."
            );
            println!("  FIX A: Fetch the missing intermediate via AIA URI in the leaf cert.");
            println!(
                "  FIX B: silksurf-app --tls-ca-file /path/to/issuer.pem https://{host}"
            );
            println!("  INFO : For corporate proxy: obtain CA from your IT/security team.");
        }
        println!(
            "  INFO : run `tls-probe {host}` after applying fix to confirm."
        );
    } else if msg.contains("InvalidCertificate") && msg.contains("Expired") {
        println!("  CAUSE: Server certificate has expired.");
        println!("  FIX  : Contact the server operator to renew the certificate.");
        println!(
            "  TEMP : silksurf-app --insecure https://{host} (DANGEROUS -- disables ALL verification)"
        );
    } else if msg.contains("InvalidCertificate") && msg.contains("NotValidYet") {
        println!(
            "  CAUSE: Server certificate not yet valid (clock skew or premature cert issuance)."
        );
        println!("  FIX A: Verify system clock is correct: date && timedatectl status");
        println!("  FIX B: Check ntpd/chrony is running: systemctl status chronyd");
    } else if msg.contains("InvalidCertificate") && msg.contains("NotValidForName") {
        println!(
            "  CAUSE: Certificate CN/SAN does not match hostname '{host}'."
        );
        println!(
            "  INFO : SNI misconfiguration, load balancer presenting wrong cert, or wrong IP."
        );
        println!("  DEBUG: compare SANs printed above against the hostname.");
    } else if msg.contains("PeerSentFatalAlert") && msg.contains("HandshakeFailure") {
        println!("  CAUSE: Server rejected TLS handshake (cipher or protocol version mismatch).");
        println!(
            "  INFO : rustls requires TLS 1.2+. Verify server is not restricted to TLS 1.0/1.1."
        );
    } else if msg.contains("NoCertificatesPresented") {
        println!(
            "  CAUSE: Server sent no certificates -- likely not a TLS service on port {}.",
            diag.total_roots
        );
        println!("  INFO : Confirm you are connecting to an HTTPS endpoint.");
    } else {
        println!("  (no specific pattern matched for this error)");
        println!("  raw error: {msg}");
        println!(
            "  mozilla_roots={} native_loaded={} native_added={}",
            diag.mozilla_roots, diag.native_certs_loaded, diag.native_certs_added
        );
    }
}

// =============================================================================
// SHA-256 -- pure-Rust FIPS 180-4 implementation for cert fingerprints
// =============================================================================

/*
 * sha256_hex -- SHA-256 fingerprint of raw bytes, returned as lowercase hex.
 *
 * WHY: TLSA matching type 1 is SHA-256 of the selector (full cert or SPKI).
 * Comparing the fingerprint of the served certificate against TLSA cert_data
 * tells the operator whether the DANE record would validate this cert.
 * We implement SHA-256 directly to avoid a new crate dependency in this tool.
 *
 * Complexity: O(N) in input size, typically <4KB per cert.
 * Correctness: FIPS 180-4 compliant; verified against known test vectors.
 */
fn sha256_hex(data: &[u8]) -> String {
    sha256(data).iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256(data: &[u8]) -> [u8; 32] {
    // Round constants: first 32 bits of fractional parts of cbrt(p) for the
    // first 64 primes. (FIPS 180-4 section 4.2.2)
    const K: [u32; 64] = [
        0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5, 0x3956_c25b, 0x59f1_11f1, 0x923f_82a4,
        0xab1c_5ed5, 0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3, 0x72be_5d74, 0x80de_b1fe,
        0x9bdc_06a7, 0xc19b_f174, 0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc, 0x2de9_2c6f,
        0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da, 0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
        0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967, 0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc,
        0x5338_0d13, 0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85, 0xa2bf_e8a1, 0xa81a_664b,
        0xc24b_8b70, 0xc76c_51a3, 0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070, 0x19a4_c116,
        0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5, 0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
        0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208, 0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7,
        0xc671_78f2,
    ];

    // Initial hash values: first 32 bits of fractional parts of sqrt(p) for
    // the first 8 primes. (FIPS 180-4 section 5.3.3)
    let mut h: [u32; 8] = [
        0x6a09_e667, 0xbb67_ae85, 0x3c6e_f372, 0xa54f_f53a, 0x510e_527f, 0x9b05_688c, 0x1f83_d9ab,
        0x5be0_cd19,
    ];

    // Pre-processing: bit-length-prefixed padding to 512-bit boundary.
    // Append 0x80, then zero bytes, then 64-bit big-endian bit length.
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    // Process each 512-bit (64-byte) block.
    for block in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] =
            [h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]];
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut result = [0u8; 32];
    for (i, &word) in h.iter().enumerate() {
        result[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    result
}

// =============================================================================
// Minimal ASN.1 DER X.509 field extractor
// =============================================================================

/*
 * asn1_cert -- decode Subject, Issuer, Validity, and SANs from a DER cert.
 *
 * WHY: Printing the issuer of each cert in the chain is the single most
 * useful piece of information for diagnosing UnknownIssuer failures. We
 * implement just enough ASN.1/DER traversal to extract these four fields
 * without pulling in a full parser crate.
 *
 * Reference: RFC 5280 section 4.1 (Certificate structure)
 *
 * IMPORTANT: this is a diagnostic tool. It does not verify signatures,
 * critical extension constraints, or certificate policies.
 */
mod asn1_cert {
    pub struct CertInfo {
        pub subject: String,
        pub issuer: String,
        pub not_before: String,
        pub not_after: String,
        pub sans: Vec<String>,
    }

    // ASN.1 universal tag numbers used in X.509 certificates.
    const SEQ: u8 = 0x30;
    const SET: u8 = 0x31;
    const OID: u8 = 0x06;
    const UTF8_STR: u8 = 0x0c;
    const PRINT_STR: u8 = 0x13;
    const IA5_STR: u8 = 0x16;
    const UTC_TIME: u8 = 0x17;
    const GEN_TIME: u8 = 0x18;
    const CTX0: u8 = 0xa0; // [0] EXPLICIT (version)
    const CTX3: u8 = 0xa3; // [3] EXPLICIT (extensions)

    // OID byte values for Name attributes we care about.
    const OID_CN: &[u8] = &[0x55, 0x04, 0x03]; // id-at-commonName
    const OID_O: &[u8] = &[0x55, 0x04, 0x0a]; // id-at-organizationName
    const OID_SAN: &[u8] = &[0x55, 0x1d, 0x11]; // id-ce-subjectAltName

    pub fn parse_cert_info(der: &[u8]) -> Option<CertInfo> {
        // Certificate ::= SEQUENCE { TBSCertificate, AlgId, Signature }
        let (tbs_seq, _) = peel_seq(der)?;
        // TBSCertificate ::= SEQUENCE { ... }
        let (tbs, _) = peel_seq(tbs_seq)?;

        let mut cur = 0usize;
        // Skip optional [0] EXPLICIT version field.
        if tbs.get(cur) == Some(&CTX0) {
            let (len, hlen) = read_len(&tbs[cur + 1..])?;
            cur += 1 + hlen + len;
        }
        // serialNumber INTEGER -- skip (non-fatal: advance past if possible)
        cur = skip_tlv(tbs, cur).unwrap_or(cur + 1);
        // signature AlgorithmIdentifier SEQUENCE -- skip (non-fatal)
        cur = skip_tlv(tbs, cur).unwrap_or(cur + 1);

        // issuer Name
        let (issuer, new_cur) = match read_tlv_at(tbs, cur) {
            Some((tlv, c)) => (
                decode_name(tlv).unwrap_or_else(|| "(?)".to_string()),
                cur + c,
            ),
            None => ("(no issuer)".to_string(), cur),
        };
        cur = new_cur;

        // validity Validity
        let (not_before, not_after, new_cur) = match read_tlv_at(tbs, cur) {
            Some((tlv, c)) => {
                let (nb, na) =
                    decode_validity(tlv).unwrap_or_else(|| ("?".to_string(), "?".to_string()));
                (nb, na, cur + c)
            }
            None => ("?".to_string(), "?".to_string(), cur),
        };
        cur = new_cur;

        // subject Name
        let (subject, new_cur) = match read_tlv_at(tbs, cur) {
            Some((tlv, c)) => (
                decode_name(tlv).unwrap_or_else(|| "(?)".to_string()),
                cur + c,
            ),
            None => ("(no subject)".to_string(), cur),
        };
        cur = new_cur;

        // subjectPublicKeyInfo -- skip (non-fatal)
        cur = skip_tlv(tbs, cur).unwrap_or(cur + 1);

        // Scan remaining TBS fields for [3] EXPLICIT extensions.
        let mut sans = Vec::new();
        while cur < tbs.len() {
            if tbs[cur] == CTX3
                && let Some((ext_wrapper, _)) = read_tlv_at(tbs, cur)
                && let Some((exts, _)) = peel_seq(ext_wrapper)
            {
                sans = parse_sans(exts);
            }
            match skip_tlv(tbs, cur) {
                Some(next) => cur = next,
                None => break,
            }
        }

        Some(CertInfo {
            subject,
            issuer,
            not_before,
            not_after,
            sans,
        })
    }

    // ---- Name (Distinguished Name) decoding ---------------------------------

    fn decode_name(der: &[u8]) -> Option<String> {
        // Name ::= SEQUENCE OF RelativeDistinguishedName
        let (seq, _) = peel_seq(der)?;
        let mut parts = Vec::new();
        let mut cur = 0;
        while cur < seq.len() {
            if seq[cur] != SET {
                cur = skip_tlv(seq, cur)?;
                continue;
            }
            let (rdn_tlv, rdn_consumed) = read_tlv_at(seq, cur)?;
            cur += rdn_consumed;
            // RDN ::= SET OF AttributeTypeAndValue
            let (rdn_body, _) = peel_tag(rdn_tlv, SET)?;
            // ATV ::= SEQUENCE { type OID, value ANY }
            let (atv_body, _) = peel_seq(rdn_body)?;
            if atv_body.first() != Some(&OID) {
                continue;
            }
            let (oid_tlv, oid_consumed) = read_tlv_at(atv_body, 0)?;
            // oid_tlv is tag + length + value; extract just the value bytes.
            let (oid_len, oid_hlen) = read_len(&oid_tlv[1..])?;
            let oid_val = &oid_tlv[1 + oid_hlen..1 + oid_hlen + oid_len];
            let (val_tlv, _) = read_tlv_at(atv_body, oid_consumed)?;
            let val_str = decode_string(val_tlv).unwrap_or_default();
            let label = match oid_val {
                v if v == OID_CN => "CN",
                v if v == OID_O => "O",
                _ => continue,
            };
            parts.push(format!("{label}={val_str}"));
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(", "))
        }
    }

    // ---- Validity -----------------------------------------------------------

    fn decode_validity(der: &[u8]) -> Option<(String, String)> {
        let (seq, _) = peel_seq(der)?;
        let (nb_tlv, nb_consumed) = read_tlv_at(seq, 0)?;
        let (na_tlv, _) = read_tlv_at(seq, nb_consumed)?;
        Some((
            decode_time(nb_tlv).unwrap_or_default(),
            decode_time(na_tlv).unwrap_or_default(),
        ))
    }

    fn decode_time(tlv: &[u8]) -> Option<String> {
        let tag = *tlv.first()?;
        let (len, hlen) = read_len(&tlv[1..])?;
        let s = std::str::from_utf8(&tlv[1 + hlen..1 + hlen + len]).ok()?;
        match tag {
            UTC_TIME if s.len() >= 12 => {
                let yy: u32 = s[..2].parse().ok()?;
                let yyyy = if yy >= 50 { 1900 + yy } else { 2000 + yy };
                Some(format!(
                    "{yyyy}-{}-{} {}:{}:{} Z",
                    &s[2..4],
                    &s[4..6],
                    &s[6..8],
                    &s[8..10],
                    &s[10..12]
                ))
            }
            GEN_TIME if s.len() >= 14 => Some(format!(
                "{}-{}-{} {}:{}:{} Z",
                &s[..4],
                &s[4..6],
                &s[6..8],
                &s[8..10],
                &s[10..12],
                &s[12..14]
            )),
            _ => Some(s.to_string()),
        }
    }

    fn decode_string(tlv: &[u8]) -> Option<String> {
        let tag = *tlv.first()?;
        let (len, hlen) = read_len(&tlv[1..])?;
        match tag {
            UTF8_STR | PRINT_STR | IA5_STR => {
                Some(String::from_utf8_lossy(&tlv[1 + hlen..1 + hlen + len]).into_owned())
            }
            _ => None,
        }
    }

    // ---- Subject Alternative Names -----------------------------------------

    /*
     * parse_sans -- extract dNSName and iPAddress from SubjectAltName extension.
     *
     * SubjectAltName extension body ::= GeneralNames
     * GeneralNames ::= SEQUENCE OF GeneralName
     * GeneralName  ::= CHOICE {
     *   rfc822Name [1], dNSName [2], iPAddress [7], ... }
     */
    fn parse_sans(extensions_seq: &[u8]) -> Vec<String> {
        let mut result = Vec::new();
        let mut cur = 0;
        while cur < extensions_seq.len() {
            let Some((ext_tlv, consumed)) = read_tlv_at(extensions_seq, cur) else {
                break;
            };
            cur += consumed;
            let Some((ext_body, _)) = peel_seq(ext_tlv) else {
                continue;
            };
            if ext_body.first() != Some(&OID) {
                continue;
            }
            let Some((oid_tlv, oid_consumed)) = read_tlv_at(ext_body, 0) else {
                continue;
            };
            // oid_tlv is tag + length + value; extract just the value bytes.
            let Some((oid_len, oid_hlen)) = read_len(&oid_tlv[1..]) else {
                continue;
            };
            if &oid_tlv[1 + oid_hlen..1 + oid_hlen + oid_len] != OID_SAN {
                continue;
            }
            // Skip optional BOOLEAN critical flag.
            let mut inner = oid_consumed;
            if ext_body.get(inner) == Some(&0x01) {
                inner = match skip_tlv(ext_body, inner) {
                    Some(n) => n,
                    None => continue,
                };
            }
            // OCTET STRING wrapping GeneralNames SEQUENCE.
            if ext_body.get(inner) != Some(&0x04) {
                continue;
            }
            let Some((octet_tlv, _)) = read_tlv_at(ext_body, inner) else {
                continue;
            };
            let Some((octet_len, octet_hlen)) = read_len(&octet_tlv[1..]) else {
                continue;
            };
            let gn_der = &octet_tlv[1 + octet_hlen..1 + octet_hlen + octet_len];
            let Some((gn_seq, _)) = peel_seq(gn_der) else {
                continue;
            };
            let mut gn = 0;
            while gn < gn_seq.len() {
                let tag = gn_seq[gn];
                let Some((len, hlen)) = read_len(&gn_seq[gn + 1..]) else {
                    break;
                };
                let val = &gn_seq[gn + 1 + hlen..gn + 1 + hlen + len];
                match tag & 0x1f {
                    2 => {
                        if let Ok(s) = std::str::from_utf8(val) {
                            result.push(s.to_string());
                        }
                    }
                    7 => match val.len() {
                        4 => result.push(format!("{}.{}.{}.{}", val[0], val[1], val[2], val[3])),
                        16 => {
                            let w: Vec<String> = val
                                .chunks(2)
                                .map(|c| format!("{:02x}{:02x}", c[0], c[1]))
                                .collect();
                            result.push(w.join(":"));
                        }
                        _ => {}
                    },
                    _ => {}
                }
                gn += 1 + hlen + len;
            }
        }
        result
    }

    // ---- Low-level DER helpers ----------------------------------------------

    fn peel_seq(der: &[u8]) -> Option<(&[u8], usize)> {
        peel_tag(der, SEQ)
    }

    fn peel_tag(der: &[u8], expected: u8) -> Option<(&[u8], usize)> {
        if der.first() != Some(&expected) {
            return None;
        }
        let (len, hlen) = read_len(&der[1..])?;
        let end = 1 + hlen + len;
        if end > der.len() {
            return None;
        }
        Some((&der[1 + hlen..end], end))
    }

    /// Return (`contents_slice`, `total_bytes_consumed`) for the TLV starting at offset.
    fn read_tlv_at(buf: &[u8], offset: usize) -> Option<(&[u8], usize)> {
        let b = &buf[offset..];
        if b.is_empty() {
            return None;
        }
        let (len, hlen) = read_len(&b[1..])?;
        let total = 1 + hlen + len;
        if total > b.len() {
            return None;
        }
        Some((&b[..total], total))
    }

    fn skip_tlv(buf: &[u8], offset: usize) -> Option<usize> {
        let (_, consumed) = read_tlv_at(buf, offset)?;
        Some(offset + consumed)
    }

    /// Returns (`length_value`, `bytes_consumed_by_length_encoding`).
    fn read_len(buf: &[u8]) -> Option<(usize, usize)> {
        let first = *buf.first()?;
        if first < 0x80 {
            Some((first as usize, 1))
        } else {
            let n = (first & 0x7f) as usize;
            if n == 0 || n > 4 || buf.len() < 1 + n {
                return None;
            }
            let mut len = 0usize;
            for &b in &buf[1..=n] {
                len = (len << 8) | b as usize;
            }
            Some((len, 1 + n))
        }
    }
}
