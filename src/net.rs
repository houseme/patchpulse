//! Shared outbound TLS policy; configured destinations never come from HTTP queries.
use std::time::Duration;
pub(crate) fn install_crypto() {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        // Concurrent initializers may select the same provider; an existing provider is valid.
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
}
pub(crate) fn client(timeout: Duration) -> anyhow::Result<reqwest::Client> {
    install_crypto();
    Ok(reqwest::Client::builder()
        .tls_backend_rustls()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .timeout(timeout)
        .connect_timeout(timeout)
        .pool_max_idle_per_host(2)
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()?)
}
