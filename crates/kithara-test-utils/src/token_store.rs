use std::{
    collections::HashMap,
    sync::{LazyLock, RwLock},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(not(target_arch = "wasm32"))]
use crate::{
    hls_spec::ResolvedHlsSpec,
    hls_stream::{GeneratedHls, load_hls},
};
use crate::{hls_url::HlsSpec, signal_spec::SignalRequest};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

static TOKEN_STORE: LazyLock<RwLock<HashMap<String, StoredToken>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

#[derive(Clone)]
pub(crate) enum StoredToken {
    Signal(SignalRequest),
    #[cfg(not(target_arch = "wasm32"))]
    Hls(Arc<GeneratedHls>),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum TokenRoute {
    Signal,
    Hls,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TokenRequest {
    pub route: TokenRoute,
    pub signal_kind: Option<String>,
    pub signal_spec_with_ext: Option<String>,
    pub hls_spec: Option<HlsSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TokenResponse {
    pub token: String,
}

pub(crate) fn insert_signal(request: SignalRequest) -> String {
    insert(StoredToken::Signal(request))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn insert_hls(spec: ResolvedHlsSpec) -> String {
    insert(StoredToken::Hls(load_hls(spec)))
}

pub(crate) fn get_signal(token: &str) -> Option<SignalRequest> {
    let store = TOKEN_STORE.read().expect("token store poisoned");
    match store.get(token) {
        Some(StoredToken::Signal(request)) => Some(request.clone()),
        _ => None,
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn get_hls(token: &str) -> Option<Arc<GeneratedHls>> {
    let store = TOKEN_STORE.read().expect("token store poisoned");
    match store.get(token) {
        Some(StoredToken::Hls(spec)) => Some(Arc::clone(spec)),
        _ => None,
    }
}

pub(crate) fn is_token(candidate: &str) -> bool {
    Uuid::parse_str(candidate).is_ok()
}

fn insert(value: StoredToken) -> String {
    let token = Uuid::new_v4().to_string();
    let mut store = TOKEN_STORE.write().expect("token store poisoned");
    store.insert(token.clone(), value);
    token
}
