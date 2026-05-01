//! TLS adapter layer for SilkSurf (cleanroom).
//!
//! Loads Mozilla root certificates (webpki-roots) plus system certificates
//! (rustls-native-certs). Provides a configured rustls ClientConfig.

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::CertificateDer;
use rustls::{ClientConfig, DigitallySignedStruct, Error, RootCertStore, SignatureScheme};
use std::fmt;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootStoreDiagnostics {
    pub mozilla_roots: usize,
    pub native_certs_loaded: usize,
    pub native_certs_added: usize,
    pub native_certs_rejected: usize,
    pub native_cert_errors: Vec<String>,
    pub total_roots: usize,
    pub ssl_cert_file: Option<String>,
    pub ssl_cert_dir: Option<String>,
    pub nix_ssl_cert_file: Option<String>,
}

#[derive(Debug)]
pub enum TlsConfigError {
    Io(std::io::Error),
    Rustls(Error),
    NoCertificates { path: PathBuf },
    NoUsableCertificates { path: PathBuf, rejected: usize },
}

impl fmt::Display for TlsConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Rustls(error) => write!(f, "{error}"),
            Self::NoCertificates { path } => {
                write!(f, "no PEM certificates found in {}", path.display())
            }
            Self::NoUsableCertificates { path, rejected } => write!(
                f,
                "no usable CA certificates found in {}; rejected {rejected}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for TlsConfigError {}

impl From<std::io::Error> for TlsConfigError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<Error> for TlsConfigError {
    fn from(error: Error) -> Self {
        Self::Rustls(error)
    }
}

#[derive(Debug, Clone)]
pub struct TlsConfig {
    inner: Arc<ClientConfig>,
}

impl TlsConfig {
    /// Create TLS config with Mozilla + system root certificates.
    pub fn new() -> Self {
        let (roots, _) = build_root_store();

        let config = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        Self {
            inner: Arc::new(config),
        }
    }

    /// Create TLS config with ALPN ["h2", "http/1.1"] for HTTP/2 negotiation.
    ///
    /// WHY: The default config has no ALPN, so servers always fall back to HTTP/1.1.
    /// This variant advertises h2 support; servers that support HTTP/2 will negotiate it.
    /// Used only by the h2 parallel-fetch path in silksurf-net; single-URL fetches
    /// via BasicClient::fetch continue to use the default (HTTP/1.1) config.
    pub fn new_h2() -> Self {
        let (roots, _) = build_root_store();
        let mut config = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Self {
            inner: Arc::new(config),
        }
    }

    /// Create TLS config with Mozilla + system roots plus a user-supplied PEM
    /// CA bundle.
    pub fn new_with_extra_ca_file(path: impl AsRef<Path>) -> Result<Self, TlsConfigError> {
        let (roots, _) = build_root_store_with_extra_ca_file(path.as_ref())?;

        let config = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        Ok(Self {
            inner: Arc::new(config),
        })
    }

    /// Create TLS config with ALPN and a user-supplied PEM CA bundle.
    pub fn new_h2_with_extra_ca_file(path: impl AsRef<Path>) -> Result<Self, TlsConfigError> {
        let (roots, _) = build_root_store_with_extra_ca_file(path.as_ref())?;

        let mut config = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Ok(Self {
            inner: Arc::new(config),
        })
    }

    /// Create TLS config using the platform verifier instead of SilkSurf's
    /// default WebPKI roots plus native-root bundle path.
    #[cfg(feature = "platform-verifier")]
    pub fn new_platform_verifier() -> Result<Self, Error> {
        use rustls_platform_verifier::BuilderVerifierExt;

        let config = ClientConfig::builder()
            .with_platform_verifier()?
            .with_no_client_auth();
        Ok(Self {
            inner: Arc::new(config),
        })
    }

    /// Create platform-verifier TLS config with ALPN ["h2", "http/1.1"].
    #[cfg(feature = "platform-verifier")]
    pub fn new_platform_verifier_h2() -> Result<Self, Error> {
        use rustls_platform_verifier::BuilderVerifierExt;

        let mut config = ClientConfig::builder()
            .with_platform_verifier()?
            .with_no_client_auth();
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Ok(Self {
            inner: Arc::new(config),
        })
    }

    /// Create platform-verifier TLS config with additional PEM roots.
    #[cfg(feature = "platform-verifier")]
    pub fn new_platform_verifier_with_extra_ca_file(
        path: impl AsRef<Path>,
    ) -> Result<Self, TlsConfigError> {
        let extra_roots = load_extra_ca_file(path.as_ref())?;
        let builder = ClientConfig::builder();
        let verifier = rustls_platform_verifier::Verifier::new_with_extra_roots(
            extra_roots,
            builder.crypto_provider().clone(),
        )?;
        let config = builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();
        Ok(Self {
            inner: Arc::new(config),
        })
    }

    /// Create platform-verifier TLS config with ALPN and additional PEM roots.
    #[cfg(feature = "platform-verifier")]
    pub fn new_platform_verifier_h2_with_extra_ca_file(
        path: impl AsRef<Path>,
    ) -> Result<Self, TlsConfigError> {
        let extra_roots = load_extra_ca_file(path.as_ref())?;
        let builder = ClientConfig::builder();
        let verifier = rustls_platform_verifier::Verifier::new_with_extra_roots(
            extra_roots,
            builder.crypto_provider().clone(),
        )?;
        let mut config = builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Ok(Self {
            inner: Arc::new(config),
        })
    }

    /// Create TLS config that accepts any certificate (INSECURE -- for debugging only).
    pub fn new_insecure() -> Self {
        let config = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoVerifier))
            .with_no_client_auth();
        Self {
            inner: Arc::new(config),
        }
    }

    /// Create insecure TLS config with ALPN ["h2", "http/1.1"].
    pub fn new_insecure_h2() -> Self {
        let mut config = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoVerifier))
            .with_no_client_auth();
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Self {
            inner: Arc::new(config),
        }
    }

    pub fn inner(&self) -> Arc<ClientConfig> {
        self.inner.clone()
    }
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self::new()
    }
}

pub fn root_store_diagnostics() -> RootStoreDiagnostics {
    build_root_store().1
}

fn build_root_store_with_extra_ca_file(
    path: &Path,
) -> Result<(RootCertStore, RootStoreDiagnostics), TlsConfigError> {
    let extra_certs = load_extra_ca_file(path)?;
    let (mut roots, diagnostics) = build_root_store();
    let (added, rejected) = roots.add_parsable_certificates(extra_certs);
    if added == 0 {
        return Err(TlsConfigError::NoUsableCertificates {
            path: path.to_path_buf(),
            rejected,
        });
    }

    Ok((roots, diagnostics))
}

fn load_extra_ca_file(path: &Path) -> Result<Vec<CertificateDer<'static>>, TlsConfigError> {
    let mut reader = BufReader::new(File::open(path)?);
    let certs = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;

    if certs.is_empty() {
        return Err(TlsConfigError::NoCertificates {
            path: path.to_path_buf(),
        });
    }

    Ok(certs)
}

fn build_root_store() -> (RootCertStore, RootStoreDiagnostics) {
    let mut roots = RootCertStore::empty();
    let mozilla_roots = webpki_roots::TLS_SERVER_ROOTS.len();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let native_result = rustls_native_certs::load_native_certs();
    let native_certs_loaded = native_result.certs.len();
    let native_cert_errors = native_result
        .errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let (native_certs_added, native_certs_rejected) =
        roots.add_parsable_certificates(native_result.certs);

    let diagnostics = RootStoreDiagnostics {
        mozilla_roots,
        native_certs_loaded,
        native_certs_added,
        native_certs_rejected,
        native_cert_errors,
        total_roots: roots.len(),
        ssl_cert_file: std::env::var("SSL_CERT_FILE").ok(),
        ssl_cert_dir: std::env::var("SSL_CERT_DIR").ok(),
        nix_ssl_cert_file: std::env::var("NIX_SSL_CERT_FILE").ok(),
    };

    (roots, diagnostics)
}

#[derive(Debug, Clone)]
pub struct RustlsProvider {
    config: TlsConfig,
    h2_config: TlsConfig,
}

impl RustlsProvider {
    pub fn new() -> Self {
        Self {
            config: TlsConfig::new(),
            h2_config: TlsConfig::new_h2(),
        }
    }

    /// Create a provider that skips certificate verification (INSECURE).
    pub fn new_insecure() -> Self {
        Self {
            config: TlsConfig::new_insecure(),
            h2_config: TlsConfig::new_insecure_h2(),
        }
    }

    /// Create a provider with extra PEM roots.
    pub fn new_with_extra_ca_file(path: impl AsRef<Path>) -> Result<Self, TlsConfigError> {
        Ok(Self {
            config: TlsConfig::new_with_extra_ca_file(path.as_ref())?,
            h2_config: TlsConfig::new_h2_with_extra_ca_file(path.as_ref())?,
        })
    }

    /// Create a provider backed by rustls-platform-verifier.
    #[cfg(feature = "platform-verifier")]
    pub fn new_platform_verifier() -> Result<Self, Error> {
        Ok(Self {
            config: TlsConfig::new_platform_verifier()?,
            h2_config: TlsConfig::new_platform_verifier_h2()?,
        })
    }

    /// Create a provider backed by rustls-platform-verifier plus extra PEM roots.
    #[cfg(feature = "platform-verifier")]
    pub fn new_platform_verifier_with_extra_ca_file(
        path: impl AsRef<Path>,
    ) -> Result<Self, TlsConfigError> {
        Ok(Self {
            config: TlsConfig::new_platform_verifier_with_extra_ca_file(path.as_ref())?,
            h2_config: TlsConfig::new_platform_verifier_h2_with_extra_ca_file(path.as_ref())?,
        })
    }
}

impl Default for RustlsProvider {
    fn default() -> Self {
        Self::new()
    }
}

pub trait TlsProvider {
    fn config(&self) -> Arc<ClientConfig>;

    /*
     * h2_config -- return a ClientConfig with ALPN ["h2", "http/1.1"].
     *
     * WHY: The default config() has no ALPN, so servers always fall back to
     * HTTP/1.1. The h2 parallel-fetch path needs a config that advertises h2
     * support so the server negotiates it. Default impl: same as config()
     * (no h2, for providers that don't support it).
     *
     * See: h2_client.rs fetch_h2_parallel -- uses this config for ALPN
     */
    fn h2_config(&self) -> Arc<ClientConfig> {
        self.config()
    }
}

impl TlsProvider for RustlsProvider {
    fn config(&self) -> Arc<ClientConfig> {
        self.config.inner()
    }

    fn h2_config(&self) -> Arc<ClientConfig> {
        self.h2_config.inner()
    }
}

/// Certificate verifier that accepts everything (DANGEROUS -- debug only).
#[derive(Debug)]
struct NoVerifier;

impl ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::ECDSA_NISTP521_SHA512,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::ED25519,
            SignatureScheme::ED448,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::root_store_diagnostics;

    #[test]
    fn root_store_diagnostics_reports_loaded_roots() {
        let diagnostics = root_store_diagnostics();

        assert!(diagnostics.mozilla_roots > 0);
        assert!(diagnostics.total_roots >= diagnostics.mozilla_roots);
        assert_eq!(
            diagnostics.total_roots,
            diagnostics.mozilla_roots + diagnostics.native_certs_added
        );
    }

    #[test]
    fn provider_h2_configs_preserve_mode_and_alpn() {
        use super::{RustlsProvider, TlsProvider};

        let default_provider = RustlsProvider::new();
        assert_eq!(
            default_provider.h2_config().alpn_protocols,
            vec![b"h2".to_vec(), b"http/1.1".to_vec()]
        );

        let insecure_provider = RustlsProvider::new_insecure();
        assert_eq!(
            insecure_provider.h2_config().alpn_protocols,
            vec![b"h2".to_vec(), b"http/1.1".to_vec()]
        );
    }
}
