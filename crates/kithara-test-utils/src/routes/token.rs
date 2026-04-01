//! Token registration route shared by `/signal/*` and `/stream/*`.

use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::post};

use crate::{
    hls_spec::resolve_hls_spec,
    signal_spec::{SignalKind, parse_signal_request},
    token_store::{TokenRequest, TokenResponse, TokenRoute, insert_hls, insert_signal},
};

pub(crate) fn router() -> Router {
    Router::new().route("/token", post(create_token))
}

async fn create_token(Json(request): Json<TokenRequest>) -> impl IntoResponse {
    match request.route {
        TokenRoute::Signal => register_signal_token(&request),
        TokenRoute::Hls => register_hls_token(request),
    }
}

fn register_signal_token(request: &TokenRequest) -> axum::response::Response {
    let Some(raw_kind) = request.signal_kind.as_deref() else {
        return (StatusCode::BAD_REQUEST, "missing `signal_kind`").into_response();
    };
    let Some(spec_with_ext) = request.signal_spec_with_ext.as_deref() else {
        return (StatusCode::BAD_REQUEST, "missing `signal_spec_with_ext`").into_response();
    };

    match SignalKind::try_from(raw_kind).and_then(|kind| parse_signal_request(kind, spec_with_ext))
    {
        Ok(signal_request) => Json(TokenResponse {
            token: insert_signal(signal_request),
        })
        .into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    }
}

fn register_hls_token(request: TokenRequest) -> axum::response::Response {
    let Some(spec) = request.hls_spec else {
        return (StatusCode::BAD_REQUEST, "missing `hls_spec`").into_response();
    };

    match resolve_hls_spec(spec) {
        Ok(spec) => Json(TokenResponse {
            token: insert_hls(spec),
        })
        .into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    }
}
