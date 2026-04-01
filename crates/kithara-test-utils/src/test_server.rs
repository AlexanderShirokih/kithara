//! Entry point for a spec-driven test server.
//!
//! Route families:
//! - `GET /health` — readiness probe for external runners
//! - `POST /token` — register spec payloads and return UUID tokens
//! - `GET /assets/{path...}` — static test assets
//! - `GET /signal/{form}/{spec_with_ext}` — procedural signal generation (sawtooth, sine, silence, …)
//! - `GET /stream/{hls_spec}` — HLS stream generation

use reqwest::Client;
use url::Url;

#[cfg(not(target_arch = "wasm32"))]
use std::env;

#[cfg(not(target_arch = "wasm32"))]
use axum::{Router, routing::get};
#[cfg(not(target_arch = "wasm32"))]
use tower_http::cors::CorsLayer;

#[cfg(not(target_arch = "wasm32"))]
use crate::http_server::TestHttpServer;
#[cfg(not(target_arch = "wasm32"))]
use crate::routes::{assets, signal, stream};
#[cfg(target_arch = "wasm32")]
use crate::server_url::join_server_url;
use crate::{
    hls_url::{
        HlsSpec, hls_init_path_from_ref, hls_key_path_from_ref, hls_master_path_from_ref,
        hls_media_path_from_ref, hls_segment_path_from_ref,
    },
    signal_url::{SignalKind, SignalSpec, signal_path},
    token_store::{TokenRequest, TokenResponse, TokenRoute},
};

/// In-process unified test server with RAII shutdown.
pub struct TestServerHelper {
    #[cfg(not(target_arch = "wasm32"))]
    server: TestHttpServer,
    #[cfg(target_arch = "wasm32")]
    base_url: Url,
}

impl TestServerHelper {
    /// Spawn the unified server on a random localhost port.
    pub async fn new() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let server = TestHttpServer::new(router()).await;
            Self { server }
        }

        #[cfg(target_arch = "wasm32")]
        {
            Self {
                base_url: external_test_server_url(),
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn join(&self, path: &str) -> Url {
        join_server_url(&self.base_url, path)
    }

    /// Build an arbitrary URL on this server.
    #[must_use]
    pub fn url(&self, path: &str) -> Url {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.server.url(path)
        }

        #[cfg(target_arch = "wasm32")]
        {
            self.join(path)
        }
    }

    /// Base URL of this server.
    #[must_use]
    pub fn base_url(&self) -> &Url {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.server.base_url()
        }

        #[cfg(target_arch = "wasm32")]
        {
            &self.base_url
        }
    }

    async fn register_signal_token(&self, kind: SignalKind, spec: &SignalSpec) -> String {
        let path = signal_path(kind, spec);
        let prefix = format!("/signal/{}/", kind.path_segment());
        let spec_with_ext = path
            .strip_prefix(&prefix)
            .expect("signal path must match kind prefix");
        let request = TokenRequest {
            route: TokenRoute::Signal,
            signal_kind: Some(kind.path_segment().to_string()),
            signal_spec_with_ext: Some(spec_with_ext.to_string()),
            hls_spec: None,
        };
        self.post_token(&request).await
    }

    pub(crate) async fn register_hls_token(&self, spec: &HlsSpec) -> String {
        let request = TokenRequest {
            route: TokenRoute::Hls,
            signal_kind: None,
            signal_spec_with_ext: None,
            hls_spec: Some(spec.clone()),
        };
        self.post_token(&request).await
    }

    async fn post_token(&self, request: &TokenRequest) -> String {
        let body = serde_json::to_vec(request).expect("token request must serialize");
        let response = Client::new()
            .post(self.url("/token"))
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .expect("token registration request must succeed")
            .error_for_status()
            .expect("token registration must return success");
        let text = response
            .text()
            .await
            .expect("token registration response must be readable");
        serde_json::from_str::<TokenResponse>(&text)
            .expect("token registration response must parse")
            .token
    }
}

#[cfg(target_arch = "wasm32")]
fn external_test_server_url() -> Url {
    let base = option_env!("TEST_SERVER_URL").unwrap_or("http://127.0.0.1:3444");
    Url::parse(base).expect("valid TEST_SERVER_URL")
}

#[cfg(not(target_arch = "wasm32"))]
async fn health() -> &'static str {
    "ok"
}

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(not(target_arch = "wasm32"))]
fn router() -> Router {
    Router::new()
        .route("/health", get(health))
        .merge(assets::router())
        .merge(signal::router())
        .merge(stream::router())
        .merge(crate::routes::token::router())
        .layer(CorsLayer::permissive())
}

impl TestServerHelper {
    /// Build a URL for a static test asset.
    ///
    /// ```ignore
    /// let url = helper.asset("hls/master.m3u8");
    /// // → http://127.0.0.1:{port}/assets/hls/master.m3u8
    /// ```
    #[must_use]
    pub fn asset(&self, name: &str) -> Url {
        let trimmed = name.trim_start_matches('/');

        #[cfg(not(target_arch = "wasm32"))]
        {
            self.server.url(&format!("/assets/{trimmed}"))
        }

        #[cfg(target_arch = "wasm32")]
        {
            self.join(&format!("/assets/{trimmed}"))
        }
    }

    /// Build a URL for `/signal/sawtooth/...`.
    #[must_use]
    pub async fn sawtooth(&self, spec: &SignalSpec) -> Url {
        let token = self.register_signal_token(SignalKind::Sawtooth, spec).await;
        self.url(&format!(
            "/signal/{}/{}.{}",
            SignalKind::Sawtooth.path_segment(),
            token,
            spec.format.path_ext()
        ))
    }

    /// Build a URL for `/signal/sawtooth-desc/...`.
    #[must_use]
    pub async fn sawtooth_descending(&self, spec: &SignalSpec) -> Url {
        let token = self
            .register_signal_token(SignalKind::SawtoothDescending, spec)
            .await;
        self.url(&format!(
            "/signal/{}/{}.{}",
            SignalKind::SawtoothDescending.path_segment(),
            token,
            spec.format.path_ext()
        ))
    }

    /// Build a URL for `/signal/sine/...`.
    #[must_use]
    pub async fn sine(&self, spec: &SignalSpec, freq_hz: f64) -> Url {
        let token = self
            .register_signal_token(SignalKind::Sine { freq_hz }, spec)
            .await;
        self.url(&format!(
            "/signal/{}/{}.{}",
            SignalKind::Sine { freq_hz }.path_segment(),
            token,
            spec.format.path_ext()
        ))
    }

    /// Build a URL for `/signal/silence/...`.
    #[must_use]
    pub async fn silence(&self, spec: &SignalSpec) -> Url {
        let token = self.register_signal_token(SignalKind::Silence, spec).await;
        self.url(&format!(
            "/signal/{}/{}.{}",
            SignalKind::Silence.path_segment(),
            token,
            spec.format.path_ext()
        ))
    }

    /// Build a URL for the canonical synthetic HLS master playlist.
    #[must_use]
    pub async fn hls(&self, spec: &HlsSpec) -> Url {
        let token = self.register_hls_token(spec).await;
        self.url(&hls_master_path_from_ref(&token))
    }

    /// Build a URL for a synthetic HLS media playlist.
    #[must_use]
    pub async fn hls_media(&self, spec: &HlsSpec, variant: usize) -> Url {
        let token = self.register_hls_token(spec).await;
        self.url(&hls_media_path_from_ref(&token, variant))
    }

    /// Build a URL for a synthetic HLS init segment.
    #[must_use]
    pub async fn hls_init(&self, spec: &HlsSpec, variant: usize) -> Url {
        let token = self.register_hls_token(spec).await;
        self.url(&hls_init_path_from_ref(&token, variant))
    }

    /// Build a URL for a synthetic HLS media segment.
    #[must_use]
    pub async fn hls_segment(&self, spec: &HlsSpec, variant: usize, segment: usize) -> Url {
        let token = self.register_hls_token(spec).await;
        self.url(&hls_segment_path_from_ref(&token, variant, segment))
    }

    /// Build a URL for a synthetic HLS key payload.
    #[must_use]
    pub async fn hls_key(&self, spec: &HlsSpec) -> Url {
        let token = self.register_hls_token(spec).await;
        self.url(&hls_key_path_from_ref(&token))
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
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
        let url = helper.sine(&spec, 440.0).await;

        assert!(url.path().starts_with("/signal/sine/"));
        assert!(url.path().ends_with(".wav"));
    }
}
