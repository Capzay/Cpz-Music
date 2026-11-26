use std::sync::Arc;
use std::time::Duration;

/// One TLS setup for every platform: ring for crypto and Mozilla's roots compiled
/// in. The OS verifier would need JNI setup on Android and differs per desktop,
/// while this behaves identically everywhere and cross-compiles without cmake.
pub fn client() -> reqwest::Client {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("ring supports the default protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth();

    reqwest::Client::builder()
        .use_preconfigured_tls(tls)
        .connect_timeout(Duration::from_secs(8))
        .build()
        .expect("a client with a preconfigured TLS config always builds")
}

/// Milliseconds since the Unix epoch, the unit the server's timestamps use.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as i64)
}
