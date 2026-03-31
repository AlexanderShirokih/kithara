//! Entry point for a spec-driven test server.
//!
//! Route families:
//! - `GET /assets/{path...}` — static test assets
//! - `GET /signal/{form}/{spec_with_ext}` — procedural signal generation (sawtooth, sine, silence, …)
//! - `GET /stream/{hls_spec}` — HLS stream generation

use std::env;

use axum::Router;
use tower_http::cors::CorsLayer;
use url::Url;

use crate::http_server::TestHttpServer;
use crate::routes::{assets, signal, stream};
use crate::signal_url::{SignalKind, SignalSpec, signal_path};

/// In-process unified test server with RAII shutdown.
pub struct TestServerHelper {
    server: TestHttpServer,
}

impl TestServerHelper {
    /// Spawn the unified server on a random localhost port.
    pub async fn new() -> Self {
        let server = TestHttpServer::new(router()).await;
        Self { server }
    }

    /// Build a URL for a static test asset.
    ///
    /// ```ignore
    /// let url = helper.asset("hls/master.m3u8");
    /// // → http://127.0.0.1:{port}/assets/hls/master.m3u8
    /// ```
    #[must_use]
    pub fn asset(&self, name: &str) -> Url {
        let trimmed = name.trim_start_matches('/');
        self.server.url(&format!("/assets/{trimmed}"))
    }

    /// Build a URL for `/signal/sawtooth/...`.
    #[must_use]
    pub fn sawtooth(&self, spec: &SignalSpec) -> Url {
        self.signal(SignalKind::Sawtooth, spec)
    }

    /// Build a URL for `/signal/sawtooth-desc/...`.
    #[must_use]
    pub fn sawtooth_descending(&self, spec: &SignalSpec) -> Url {
        self.signal(SignalKind::SawtoothDescending, spec)
    }

    /// Build a URL for `/signal/sine/...`.
    #[must_use]
    pub fn sine(&self, spec: &SignalSpec, freq_hz: f64) -> Url {
        self.signal(SignalKind::Sine { freq_hz }, spec)
    }

    /// Build a URL for `/signal/silence/...`.
    #[must_use]
    pub fn silence(&self, spec: &SignalSpec) -> Url {
        self.signal(SignalKind::Silence, spec)
    }

    fn signal(&self, kind: SignalKind, spec: &SignalSpec) -> Url {
        self.server.url(&signal_path(kind, spec))
    }

    /// Build an arbitrary URL on this server.
    #[must_use]
    pub fn url(&self, path: &str) -> Url {
        self.server.url(path)
    }

    /// Base URL of this server.
    #[must_use]
    pub fn base_url(&self) -> &Url {
        self.server.base_url()
    }
}

/// Start the server as a standalone process (used by the `test_server` binary).
pub async fn run_test_server() {
    let port: u16 = env::var("TEST_SERVER_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3444);

    let mut server = TestHttpServer::bind(&format!("0.0.0.0:{port}"), router()).await;
    println!("test server listening on {}", server.base_url());
    server.completion().await;
}

fn router() -> Router {
    Router::new()
        .merge(assets::router())
        .merge(signal::router())
        .merge(stream::router())
        .layer(CorsLayer::permissive())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal_url::{SignalFormat, SignalSpecLength};

    #[tokio::test]
    async fn signal_helper_builds_expected_url() {
        let spec = SignalSpec {
            sample_rate: 44_100,
            channels: 2,
            length: SignalSpecLength::Seconds(1.0),
            format: SignalFormat::Wav,
        };
        let helper = TestServerHelper::new().await;
        let url = helper.sine(&spec, 440.0);

        assert_eq!(
            url.path(),
            signal_path(SignalKind::Sine { freq_hz: 440.0 }, &spec)
        );
    }
}
