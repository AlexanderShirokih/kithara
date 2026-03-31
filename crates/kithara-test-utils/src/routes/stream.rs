//! # HLS stream generation route.
//!
//! Provides access to synthetic HLS streams.
//!
//! ## Routes:
//! - `GET /stream/{hls_spec}.m3u8` — synthetic HLS stream generation.
//! - `GET /stream/{hls_spec}/v{variant}.m3u8`          — media playlist
//! - `GET /stream/{hls_spec}/init/v{variant}.mp4`      — init segment
//! - `GET /stream/{hls_spec}/seg/v{variant}_{seg}.m4s` — media fragment
//! - `HEAD /stream/{hls_spec}/seg/v{variant}_{seg}.m4s` — fragment size

use axum::{Router, extract::Path, http::StatusCode, response::IntoResponse, routing::get};

pub(crate) fn router() -> Router {
    Router::new()
        .route("/stream/{hls_spec}", get(master_playlist))
        .route("/stream/{hls_spec}/{media_playlist}", get(media_playlist))
        .route("/stream/{hls_spec}/init/{init_segment}", get(init_segment))
        .route(
            "/stream/{hls_spec}/seg/{media_segment}",
            get(media_segment).head(media_segment),
        )
}

async fn master_playlist(Path(hls_spec): Path<String>) -> impl IntoResponse {
    let _spec_b64 = hls_spec.strip_suffix(".m3u8").unwrap_or(&hls_spec);
    // TODO: decode base64url spec, generate master playlist
    StatusCode::NOT_IMPLEMENTED.into_response()
}

async fn media_playlist(
    Path((hls_spec, media_playlist)): Path<(String, String)>,
) -> impl IntoResponse {
    let _ = (hls_spec, media_playlist);
    // TODO: decode spec, generate media playlist for variant
    StatusCode::NOT_IMPLEMENTED.into_response()
}

async fn init_segment(Path((hls_spec, init_segment)): Path<(String, String)>) -> impl IntoResponse {
    let _ = (hls_spec, init_segment);
    // TODO: decode spec, generate init.mp4 for variant
    StatusCode::NOT_IMPLEMENTED.into_response()
}

async fn media_segment(
    Path((hls_spec, media_segment)): Path<(String, String)>,
) -> impl IntoResponse {
    let _ = (hls_spec, media_segment);
    // TODO: decode spec, generate .m4s for variant+segment
    StatusCode::NOT_IMPLEMENTED.into_response()
}
