use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, RwLock},
};

use sha2::{Digest, Sha256};

#[cfg(not(target_arch = "wasm32"))]
static HLS_BLOBS: LazyLock<RwLock<HashMap<String, Arc<Vec<u8>>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Store an HLS payload blob and return a stable content-addressed key.
#[must_use]
pub fn register_hls_blob(bytes: &[u8]) -> String {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = bytes;
        panic!("register_hls_blob is only supported on native tests");
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let hash = Sha256::digest(bytes);
        let key = format!("sha256:{}", hex::encode(hash));
        let mut blobs = HLS_BLOBS.write().expect("hls blob store poisoned");
        blobs
            .entry(key.clone())
            .or_insert_with(|| Arc::new(bytes.to_vec()));
        key
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn resolve_hls_blob(key: &str) -> Option<Arc<Vec<u8>>> {
    let blobs = HLS_BLOBS.read().expect("hls blob store poisoned");
    blobs.get(key).cloned()
}
