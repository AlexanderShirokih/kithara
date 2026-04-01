//! Minimal HLS fixture helpers backed by unified `/stream/*` routes.

use std::{sync::Arc, time::Duration};

use url::Url;

use crate::{
    HlsSpec, TestServerHelper,
    fixture_protocol::{DataMode, DelayRule, EncryptionRequest, InitMode},
    hls_url::{
        hls_init_path_from_ref, hls_key_path_from_ref, hls_master_path_from_ref,
        hls_media_path_from_ref, hls_segment_path_from_ref,
    },
    register_hls_blob,
};

/// Fixed the three-variant HLS fixture used by many integration tests.
pub struct TestServer {
    helper: TestServerHelper,
    plain_token: String,
    init_token: String,
    encrypted_token: String,
}

impl TestServer {
    #[must_use]
    pub async fn new() -> Self {
        let helper = TestServerHelper::new().await;
        let plain_token = helper.register_hls_token(&fixed_plain_spec()).await;
        let init_token = helper.register_hls_token(&fixed_init_spec()).await;
        let encrypted_token = helper.register_hls_token(&fixed_encrypted_spec()).await;
        Self {
            helper,
            plain_token,
            init_token,
            encrypted_token,
        }
    }

    #[must_use]
    pub fn url(&self, path: &str) -> Url {
        match path {
            "/master.m3u8" => self
                .helper
                .url(&hls_master_path_from_ref(&self.plain_token)),
            "/master-init.m3u8" => self.helper.url(&hls_master_path_from_ref(&self.init_token)),
            "/master-encrypted.m3u8" => self
                .helper
                .url(&hls_master_path_from_ref(&self.encrypted_token)),
            "/v0.m3u8" => self
                .helper
                .url(&hls_media_path_from_ref(&self.plain_token, 0)),
            "/v1.m3u8" | "/video/480p/playlist.m3u8" => self
                .helper
                .url(&hls_media_path_from_ref(&self.plain_token, 1)),
            "/v2.m3u8" => self
                .helper
                .url(&hls_media_path_from_ref(&self.plain_token, 2)),
            "/v0-init.m3u8" => self
                .helper
                .url(&hls_media_path_from_ref(&self.init_token, 0)),
            "/v1-init.m3u8" => self
                .helper
                .url(&hls_media_path_from_ref(&self.init_token, 1)),
            "/v2-init.m3u8" => self
                .helper
                .url(&hls_media_path_from_ref(&self.init_token, 2)),
            "/v0-encrypted.m3u8" => self
                .helper
                .url(&hls_media_path_from_ref(&self.encrypted_token, 0)),
            "/seg/v0_0.bin" => self
                .helper
                .url(&hls_segment_path_from_ref(&self.plain_token, 0, 0)),
            "/seg/v0_1.bin" => self
                .helper
                .url(&hls_segment_path_from_ref(&self.plain_token, 0, 1)),
            "/seg/v0_2.bin" => self
                .helper
                .url(&hls_segment_path_from_ref(&self.plain_token, 0, 2)),
            "/seg/v1_0.bin" => self
                .helper
                .url(&hls_segment_path_from_ref(&self.plain_token, 1, 0)),
            "/seg/v1_1.bin" => self
                .helper
                .url(&hls_segment_path_from_ref(&self.plain_token, 1, 1)),
            "/seg/v1_2.bin" => self
                .helper
                .url(&hls_segment_path_from_ref(&self.plain_token, 1, 2)),
            "/seg/v2_0.bin" => self
                .helper
                .url(&hls_segment_path_from_ref(&self.plain_token, 2, 0)),
            "/seg/v2_1.bin" => self
                .helper
                .url(&hls_segment_path_from_ref(&self.plain_token, 2, 1)),
            "/seg/v2_2.bin" => self
                .helper
                .url(&hls_segment_path_from_ref(&self.plain_token, 2, 2)),
            "/init/v0.bin" => self
                .helper
                .url(&hls_init_path_from_ref(&self.init_token, 0)),
            "/init/v1.bin" => self
                .helper
                .url(&hls_init_path_from_ref(&self.init_token, 1)),
            "/init/v2.bin" => self
                .helper
                .url(&hls_init_path_from_ref(&self.init_token, 2)),
            "/key.bin" => self.helper.url(&hls_key_path_from_ref(&self.plain_token)),
            "/aes/key.bin" => self
                .helper
                .url(&hls_key_path_from_ref(&self.encrypted_token)),
            "/aes/seg0.bin" => {
                self.helper
                    .url(&hls_segment_path_from_ref(&self.encrypted_token, 0, 0))
            }
            other => self.helper.url(other),
        }
    }
}

#[crate::kithara::fixture]
pub async fn test_server() -> TestServer {
    TestServer::new().await
}

#[must_use]
pub fn test_master_playlist() -> &'static str {
    r#"#EXTM3U
#EXT-X-VERSION:6
#EXT-X-STREAM-INF:BANDWIDTH=1280000,RESOLUTION=854x480,CODECS="avc1.42c01e,mp4a.40.2"
v0.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=2560000,RESOLUTION=1280x720,CODECS="avc1.42c01e,mp4a.40.2"
v1.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=5120000,RESOLUTION=1920x1080,CODECS="avc1.42c01e,mp4a.40.2"
v2.m3u8
"#
}

#[must_use]
pub fn test_master_playlist_with_init() -> &'static str {
    r#"#EXTM3U
#EXT-X-VERSION:6
#EXT-X-STREAM-INF:BANDWIDTH=1280000,RESOLUTION=854x480,CODECS="avc1.42c01e,mp4a.40.2"
v0-init.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=2560000,RESOLUTION=1280x720,CODECS="avc1.42c01e,mp4a.40.2"
v1-init.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=5120000,RESOLUTION=1920x1080,CODECS="avc1.42c01e,mp4a.40.2"
v2-init.m3u8
"#
}

#[must_use]
pub fn test_media_playlist(variant: usize) -> String {
    format!(
        r#"#EXTM3U
#EXT-X-VERSION:6
#EXT-X-TARGETDURATION:4
#EXT-X-MEDIA-SEQUENCE:0
#EXT-X-PLAYLIST-TYPE:VOD
#EXTINF:4.0,
seg/v{}_0.bin
#EXTINF:4.0,
seg/v{}_1.bin
#EXTINF:4.0,
seg/v{}_2.bin
#EXT-X-ENDLIST
"#,
        variant, variant, variant
    )
}

#[must_use]
pub fn test_media_playlist_with_init(variant: usize) -> String {
    format!(
        r#"#EXTM3U
#EXT-X-VERSION:6
#EXT-X-TARGETDURATION:4
#EXT-X-MEDIA-SEQUENCE:0
#EXT-X-PLAYLIST-TYPE:VOD
#EXT-X-MAP:URI="init/v{}.bin"
#EXTINF:4.0,
seg/v{}_0.bin
#EXTINF:4.0,
seg/v{}_1.bin
#EXTINF:4.0,
seg/v{}_2.bin
#EXT-X-ENDLIST
"#,
        variant, variant, variant, variant
    )
}

#[must_use]
pub fn test_segment_data(variant: usize, segment: usize) -> Vec<u8> {
    let prefix = format!("V{variant}-SEG-{segment}:");
    let mut data = prefix.into_bytes();
    data.extend(b"TEST_SEGMENT_DATA");
    if data.len() < 200_000 {
        data.resize(200_000, 0xFF);
    }
    data
}

#[must_use]
pub fn test_master_playlist_encrypted() -> &'static str {
    r#"#EXTM3U
#EXT-X-VERSION:6
#EXT-X-STREAM-INF:BANDWIDTH=1280000,RESOLUTION=854x480,CODECS="avc1.42c01e,mp4a.40.2"
v0-encrypted.m3u8
"#
}

#[must_use]
pub fn test_media_playlist_encrypted(_variant: usize) -> String {
    r#"#EXTM3U
#EXT-X-VERSION:6
#EXT-X-TARGETDURATION:4
#EXT-X-MEDIA-SEQUENCE:0
#EXT-X-PLAYLIST-TYPE:VOD
#EXT-X-KEY:METHOD=AES-128,URI="../aes/key.bin",IV=0x00000000000000000000000000000000
#EXTINF:4.0,
../aes/seg0.bin
#EXT-X-ENDLIST
"#
    .to_string()
}

/// AES-128 encryption configuration for configurable HLS fixtures.
pub struct EncryptionConfig {
    pub key: [u8; 16],
    pub iv: Option<[u8; 16]>,
}

/// Configuration for [`HlsTestServer`].
pub struct HlsTestServerConfig {
    pub variant_count: usize,
    pub segments_per_variant: usize,
    pub segment_size: usize,
    pub segment_duration_secs: f64,
    pub custom_data: Option<Arc<Vec<u8>>>,
    pub custom_data_per_variant: Option<Vec<Arc<Vec<u8>>>>,
    pub init_data_per_variant: Option<Vec<Arc<Vec<u8>>>>,
    pub variant_bandwidths: Option<Vec<u64>>,
    pub delay_rules: Vec<DelayRule>,
    pub encryption: Option<EncryptionConfig>,
    pub head_reported_segment_size: Option<usize>,
}

impl Default for HlsTestServerConfig {
    fn default() -> Self {
        Self {
            variant_count: 1,
            segments_per_variant: 3,
            segment_size: 200_000,
            segment_duration_secs: 4.0,
            custom_data: None,
            custom_data_per_variant: None,
            init_data_per_variant: None,
            variant_bandwidths: None,
            delay_rules: Vec::new(),
            encryption: None,
            head_reported_segment_size: None,
        }
    }
}

/// Configurable HLS fixture backed by the unified synthetic stream routes.
pub struct HlsTestServer {
    config: HlsTestServerConfig,
    helper: TestServerHelper,
    token: String,
}

impl HlsTestServer {
    #[must_use]
    pub async fn new(config: HlsTestServerConfig) -> Self {
        let helper = TestServerHelper::new().await;
        let spec = spec_from_config(&config);
        let token = helper.register_hls_token(&spec).await;
        Self {
            config,
            helper,
            token,
        }
    }

    #[must_use]
    pub fn url(&self, path: &str) -> Url {
        match path {
            "/master.m3u8" => self.helper.url(&hls_master_path_from_ref(&self.token)),
            "/key.bin" => self.helper.url(&hls_key_path_from_ref(&self.token)),
            other if other.starts_with("/playlist/v") && other.ends_with(".m3u8") => {
                let variant = parse_variant(other, "/playlist/v", ".m3u8").unwrap_or(0);
                self.helper
                    .url(&hls_media_path_from_ref(&self.token, variant))
            }
            other if other.starts_with("/seg/v") && other.ends_with(".bin") => {
                let (variant, segment) = parse_segment(other).unwrap_or((0, 0));
                self.helper
                    .url(&hls_segment_path_from_ref(&self.token, variant, segment))
            }
            other if other.starts_with("/init/v") && other.ends_with("_init.bin") => {
                let variant = parse_variant(other, "/init/v", "_init.bin").unwrap_or(0);
                self.helper
                    .url(&hls_init_path_from_ref(&self.token, variant))
            }
            other => self.helper.url(other),
        }
    }

    #[must_use]
    pub fn config(&self) -> &HlsTestServerConfig {
        &self.config
    }

    #[must_use]
    pub fn init_len(&self) -> u64 {
        self.config
            .init_data_per_variant
            .as_ref()
            .and_then(|d| d.first())
            .map_or(0, |d| d.len() as u64)
    }

    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.init_len() + self.config.segments_per_variant as u64 * self.config.segment_size as u64
    }

    #[must_use]
    pub fn total_duration_secs(&self) -> f64 {
        self.config.segments_per_variant as f64 * self.config.segment_duration_secs
    }

    #[must_use]
    pub fn expected_byte_at(&self, variant: usize, offset: u64) -> u8 {
        let init_len = self
            .config
            .init_data_per_variant
            .as_ref()
            .and_then(|d| d.get(variant))
            .map_or(0u64, |d| d.len() as u64);

        if offset < init_len {
            return self
                .config
                .init_data_per_variant
                .as_ref()
                .and_then(|data| data.get(variant))
                .and_then(|data| data.get(offset as usize))
                .copied()
                .unwrap_or(0);
        }

        let media_offset = offset - init_len;
        if let Some(ref per_variant) = self.config.custom_data_per_variant
            && let Some(data) = per_variant.get(variant)
        {
            return data.get(media_offset as usize).copied().unwrap_or(0);
        }
        if let Some(data) = &self.config.custom_data {
            return data.get(media_offset as usize).copied().unwrap_or(0);
        }

        let seg_idx = (media_offset / self.config.segment_size as u64) as usize;
        let off_in_seg = (media_offset % self.config.segment_size as u64) as usize;
        let prefix = format!("V{variant}-SEG-{seg_idx}:TEST_SEGMENT_DATA");
        let prefix_bytes = prefix.as_bytes();
        if off_in_seg < prefix_bytes.len() {
            prefix_bytes[off_in_seg]
        } else {
            0xFF
        }
    }
}

/// ABR-specific helpers preserved for existing tests.
pub mod abr {
    pub use super::{AbrTestServer, master_playlist};
}

/// ABR fixture backed by the unified synthetic stream routes.
pub struct AbrTestServer {
    helper: TestServerHelper,
    token: String,
}

impl AbrTestServer {
    #[must_use]
    pub async fn new(master_playlist: String, init: bool, segment0_delay: Duration) -> Self {
        let helper = TestServerHelper::new().await;
        let spec = abr_spec(&master_playlist, init, segment0_delay);
        let token = helper.register_hls_token(&spec).await;
        Self { helper, token }
    }

    #[must_use]
    pub fn url(&self, path: &str) -> Url {
        match path {
            "/master.m3u8" => self.helper.url(&hls_master_path_from_ref(&self.token)),
            "/v0.m3u8" => self.helper.url(&hls_media_path_from_ref(&self.token, 0)),
            "/v1.m3u8" => self.helper.url(&hls_media_path_from_ref(&self.token, 1)),
            "/v2.m3u8" => self.helper.url(&hls_media_path_from_ref(&self.token, 2)),
            "/seg/v0_0.bin" => self
                .helper
                .url(&hls_segment_path_from_ref(&self.token, 0, 0)),
            "/seg/v0_1.bin" => self
                .helper
                .url(&hls_segment_path_from_ref(&self.token, 0, 1)),
            "/seg/v0_2.bin" => self
                .helper
                .url(&hls_segment_path_from_ref(&self.token, 0, 2)),
            "/seg/v1_0.bin" => self
                .helper
                .url(&hls_segment_path_from_ref(&self.token, 1, 0)),
            "/seg/v1_1.bin" => self
                .helper
                .url(&hls_segment_path_from_ref(&self.token, 1, 1)),
            "/seg/v1_2.bin" => self
                .helper
                .url(&hls_segment_path_from_ref(&self.token, 1, 2)),
            "/seg/v2_0.bin" => self
                .helper
                .url(&hls_segment_path_from_ref(&self.token, 2, 0)),
            "/seg/v2_1.bin" => self
                .helper
                .url(&hls_segment_path_from_ref(&self.token, 2, 1)),
            "/seg/v2_2.bin" => self
                .helper
                .url(&hls_segment_path_from_ref(&self.token, 2, 2)),
            "/init/v0.bin" => self.helper.url(&hls_init_path_from_ref(&self.token, 0)),
            "/init/v1.bin" => self.helper.url(&hls_init_path_from_ref(&self.token, 1)),
            "/init/v2.bin" => self.helper.url(&hls_init_path_from_ref(&self.token, 2)),
            other => self.helper.url(other),
        }
    }
}

#[must_use]
pub fn master_playlist(v0_bw: u64, v1_bw: u64, v2_bw: u64) -> String {
    format!(
        r#"#EXTM3U
#EXT-X-VERSION:6
#EXT-X-STREAM-INF:BANDWIDTH={v0_bw}
v0.m3u8
#EXT-X-STREAM-INF:BANDWIDTH={v1_bw}
v1.m3u8
#EXT-X-STREAM-INF:BANDWIDTH={v2_bw}
v2.m3u8
"#
    )
}

fn fixed_plain_spec() -> HlsSpec {
    HlsSpec {
        variant_count: 3,
        key_hex: Some(hex::encode(test_key_data())),
        ..HlsSpec::default()
    }
}

fn fixed_init_spec() -> HlsSpec {
    HlsSpec {
        variant_count: 3,
        init_mode: InitMode::TestInit,
        ..fixed_plain_spec()
    }
}

fn fixed_encrypted_spec() -> HlsSpec {
    HlsSpec {
        variant_count: 1,
        segments_per_variant: 1,
        segment_size: aes128_plaintext_segment().len(),
        data_mode: DataMode::CustomDataPerVariant(vec![aes128_plaintext_segment()]),
        encryption: Some(EncryptionRequest {
            key_hex: hex::encode(aes128_key_bytes()),
            iv_hex: Some(hex::encode(aes128_iv())),
        }),
        head_reported_segment_size: Some(aes128_plaintext_segment().len()),
        ..HlsSpec::default()
    }
}

fn spec_from_config(config: &HlsTestServerConfig) -> HlsSpec {
    let data_mode = config.custom_data_per_variant.as_ref().map_or_else(
        || {
            config
                .custom_data
                .as_ref()
                .map_or(DataMode::TestPattern, |data| {
                    DataMode::BlobRef(register_hls_blob(data))
                })
        },
        |per_variant| {
            DataMode::BlobRefs(
                per_variant
                    .iter()
                    .map(|bytes| register_hls_blob(bytes))
                    .collect(),
            )
        },
    );

    let init_mode = config
        .init_data_per_variant
        .as_ref()
        .map_or(InitMode::None, |data| {
            InitMode::BlobRefs(data.iter().map(|bytes| register_hls_blob(bytes)).collect())
        });

    HlsSpec {
        variant_count: config.variant_count,
        segments_per_variant: config.segments_per_variant,
        segment_size: config.segment_size,
        segment_duration_secs: config.segment_duration_secs,
        data_mode,
        init_mode,
        variant_bandwidths: config.variant_bandwidths.clone(),
        delay_rules: config.delay_rules.clone(),
        encryption: config.encryption.as_ref().map(|enc| EncryptionRequest {
            key_hex: hex::encode(enc.key),
            iv_hex: enc.iv.map(hex::encode),
        }),
        head_reported_segment_size: config
            .head_reported_segment_size
            .or_else(|| config.encryption.as_ref().map(|_| config.segment_size)),
        key_hex: None,
        key_blob_ref: None,
    }
}

fn abr_spec(master_playlist: &str, init: bool, segment0_delay: Duration) -> HlsSpec {
    let variant_bandwidths = parse_master_bandwidths(master_playlist);
    let init_mode = if init {
        InitMode::Custom((0..3).map(abr_init_data).collect())
    } else {
        InitMode::None
    };

    HlsSpec {
        variant_count: variant_bandwidths.len().max(1),
        data_mode: DataMode::AbrBinary,
        init_mode,
        variant_bandwidths: Some(variant_bandwidths),
        delay_rules: vec![DelayRule {
            variant: Some(2),
            segment_eq: Some(0),
            segment_gte: None,
            delay_ms: segment0_delay.as_millis() as u64,
        }],
        head_reported_segment_size: Some(200_000),
        ..HlsSpec::default()
    }
}

fn parse_master_bandwidths(master_playlist: &str) -> Vec<u64> {
    master_playlist
        .lines()
        .filter_map(|line| line.strip_prefix("#EXT-X-STREAM-INF:BANDWIDTH="))
        .filter_map(|value| value.split(',').next())
        .filter_map(|value| value.parse::<u64>().ok())
        .collect()
}

fn parse_variant(path: &str, prefix: &str, suffix: &str) -> Option<usize> {
    path.strip_prefix(prefix)?
        .strip_suffix(suffix)?
        .parse()
        .ok()
}

fn parse_segment(path: &str) -> Option<(usize, usize)> {
    let path = path.strip_prefix("/seg/v")?.strip_suffix(".bin")?;
    let (variant, segment) = path.split_once('_')?;
    Some((variant.parse().ok()?, segment.parse().ok()?))
}

fn abr_init_data(variant: usize) -> Vec<u8> {
    format!("V{variant}-INIT:").into_bytes()
}

fn aes128_key_bytes() -> Vec<u8> {
    b"0123456789abcdef".to_vec()
}

fn aes128_iv() -> [u8; 16] {
    [0u8; 16]
}

fn aes128_plaintext_segment() -> Vec<u8> {
    b"V0-SEG-0:DRM-PLAINTEXT".to_vec()
}

fn test_key_data() -> Vec<u8> {
    b"TEST_KEY_DATA_123456".to_vec()
}
