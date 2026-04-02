use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use aes::Aes128;
use cbc::{
    Encryptor,
    cipher::{BlockEncryptMut, KeyIvInit, block_padding::Pkcs7},
};

use crate::{
    fixture_protocol::{create_wav_init_header, generate_segment},
    hls_spec::{ResolvedDataMode, ResolvedEncryption, ResolvedHlsSpec, ResolvedInitMode},
    signal_pcm::{Finite, SignalPcm, signal},
    wav::create_wav_from_signal,
};

pub(crate) type GeneratedHlsCache = RwLock<HashMap<String, Arc<GeneratedHls>>>;

pub(crate) fn load_hls(cache: &GeneratedHlsCache, spec: ResolvedHlsSpec) -> Arc<GeneratedHls> {
    let cache_key = spec.cache_key().to_owned();
    {
        let cache = cache.read().expect("hls cache poisoned");
        if let Some(existing) = cache.get(&cache_key) {
            return Arc::clone(existing);
        }
    }

    let generated = Arc::new(GeneratedHls::new(spec));
    let mut cache = cache.write().expect("hls cache poisoned");
    Arc::clone(
        cache
            .entry(cache_key)
            .or_insert_with(|| Arc::clone(&generated)),
    )
}

pub(crate) struct GeneratedHls {
    spec: ResolvedHlsSpec,
    master_playlist: String,
    media_playlists: Vec<String>,
    data_mode: MaterializedDataMode,
    init_segments: Vec<Arc<Vec<u8>>>,
}

enum MaterializedDataMode {
    TestPattern,
    AbrBinary,
    SharedBytes(Arc<Vec<u8>>),
    PerVariantBytes(Vec<Arc<Vec<u8>>>),
}

impl GeneratedHls {
    fn new(spec: ResolvedHlsSpec) -> Self {
        let data_mode = materialize_data_mode(&spec);
        let init_segments = materialize_init_mode(&spec);
        let master_playlist = build_master_playlist(&spec);
        let media_playlists = (0..spec.variant_count)
            .map(|variant| build_media_playlist(&spec, variant))
            .collect();

        Self {
            spec,
            master_playlist,
            media_playlists,
            data_mode,
            init_segments,
        }
    }

    pub(crate) fn master_playlist(&self, encoded_spec: &str) -> String {
        self.master_playlist.replace("{spec}", encoded_spec)
    }

    pub(crate) fn media_playlist(&self, variant: usize) -> Option<&str> {
        self.media_playlists.get(variant).map(String::as_str)
    }

    pub(crate) fn key_bytes(&self) -> Option<Vec<u8>> {
        self.spec
            .key_data
            .as_ref()
            .map(|bytes| bytes.as_slice().to_vec())
            .or_else(|| {
                self.spec
                    .encryption
                    .as_ref()
                    .map(|enc| enc.key.as_slice().to_vec())
            })
    }

    pub(crate) fn init_bytes(&self, variant: usize) -> Option<Vec<u8>> {
        let plaintext = self.init_segments.get(variant)?.as_slice();
        Some(self.encrypt_if_needed(plaintext, 0))
    }

    pub(crate) fn segment_len(
        &self,
        variant: usize,
        segment: usize,
        use_head_override: bool,
    ) -> Option<usize> {
        self.segment_plaintext(variant, segment).map(|plaintext| {
            if use_head_override {
                self.spec
                    .head_reported_segment_size
                    .unwrap_or(plaintext.len())
            } else if self.spec.encryption.is_some() {
                plaintext.len() + (16 - plaintext.len() % 16)
            } else {
                plaintext.len()
            }
        })
    }

    pub(crate) fn segment_bytes(&self, variant: usize, segment: usize) -> Option<Vec<u8>> {
        let plaintext = self.segment_plaintext(variant, segment)?;
        Some(self.encrypt_if_needed(&plaintext, segment))
    }

    pub(crate) fn segment_delay_ms(&self, variant: usize, segment: usize) -> u64 {
        self.spec
            .delay_rules
            .iter()
            .find_map(|rule| rule.matches(variant, segment))
            .unwrap_or(0)
    }

    fn segment_plaintext(&self, variant: usize, segment: usize) -> Option<Vec<u8>> {
        if variant >= self.spec.variant_count || segment >= self.spec.segments_per_variant {
            return None;
        }

        let start = segment.checked_mul(self.spec.segment_size)?;
        match &self.data_mode {
            MaterializedDataMode::TestPattern => {
                Some(generate_segment(variant, segment, self.spec.segment_size))
            }
            MaterializedDataMode::AbrBinary => Some(generate_abr_binary_segment(variant, segment)),
            MaterializedDataMode::SharedBytes(bytes) => {
                let end = (start + self.spec.segment_size).min(bytes.len());
                Some(bytes.get(start..end).unwrap_or(&[]).to_vec())
            }
            MaterializedDataMode::PerVariantBytes(per_variant) => {
                let bytes = per_variant.get(variant)?;
                let end = (start + self.spec.segment_size).min(bytes.len());
                Some(bytes.get(start..end).unwrap_or(&[]).to_vec())
            }
        }
    }

    fn encrypt_if_needed(&self, data: &[u8], sequence: usize) -> Vec<u8> {
        let Some(enc) = &self.spec.encryption else {
            return data.to_vec();
        };
        let iv = derive_iv(enc, sequence);
        encrypt_aes128_cbc(data, &enc.key, &iv)
    }
}

fn materialize_data_mode(spec: &ResolvedHlsSpec) -> MaterializedDataMode {
    match &spec.data_mode {
        ResolvedDataMode::TestPattern => MaterializedDataMode::TestPattern,
        ResolvedDataMode::AbrBinary => MaterializedDataMode::AbrBinary,
        ResolvedDataMode::SharedBytes(bytes) => {
            MaterializedDataMode::SharedBytes(Arc::clone(bytes))
        }
        ResolvedDataMode::PerVariantBytes(bytes) => {
            MaterializedDataMode::PerVariantBytes(bytes.clone())
        }
        ResolvedDataMode::SawWav {
            sample_rate,
            channels,
        } => {
            let wav = create_wav_from_signal(SignalPcm::new(
                signal::Sawtooth,
                *sample_rate,
                *channels,
                Finite::from_segments(spec.segments_per_variant, spec.segment_size, *channels),
            ));
            MaterializedDataMode::SharedBytes(Arc::new(wav))
        }
        ResolvedDataMode::PerVariantPcm {
            sample_rate,
            channels,
            patterns,
        } => {
            let bytes = (0..spec.variant_count)
                .map(|variant| {
                    let pattern = patterns
                        .get(variant)
                        .cloned()
                        .unwrap_or(crate::fixture_protocol::PcmPattern::Ascending);
                    Arc::new(
                        SignalPcm::new(
                            pattern,
                            *sample_rate,
                            *channels,
                            Finite::from_segments(
                                spec.segments_per_variant,
                                spec.segment_size,
                                *channels,
                            ),
                        )
                        .into_vec(),
                    )
                })
                .collect();
            MaterializedDataMode::PerVariantBytes(bytes)
        }
    }
}

fn generate_abr_binary_segment(variant: usize, segment: usize) -> Vec<u8> {
    let total_len: usize = if variant == 2 && segment == 0 {
        50_000
    } else {
        200_000
    };
    let header_size = 1 + 4 + 4;
    let data_len = total_len.saturating_sub(header_size);

    let mut data = Vec::with_capacity(total_len);
    data.push(variant as u8);
    data.extend(&(segment as u32).to_be_bytes());
    data.extend(&(data_len as u32).to_be_bytes());
    data.extend(std::iter::repeat_n(b'A', data_len));
    data
}

fn materialize_init_mode(spec: &ResolvedHlsSpec) -> Vec<Arc<Vec<u8>>> {
    match &spec.init_mode {
        ResolvedInitMode::None => (0..spec.variant_count)
            .map(|_| Arc::new(Vec::new()))
            .collect(),
        ResolvedInitMode::TestInit => (0..spec.variant_count)
            .map(|variant| Arc::new(generate_test_init_segment(variant)))
            .collect(),
        ResolvedInitMode::WavHeader {
            sample_rate,
            channels,
        } => {
            let header = Arc::new(create_wav_init_header(*sample_rate, *channels));
            vec![header; spec.variant_count]
        }
        ResolvedInitMode::PerVariantBytes(data) => (0..spec.variant_count)
            .map(|variant| {
                data.get(variant)
                    .cloned()
                    .unwrap_or_else(|| Arc::new(Vec::new()))
            })
            .collect(),
    }
}

fn generate_test_init_segment(variant: usize) -> Vec<u8> {
    let prefix = format!("V{variant}-INIT:");
    let mut data = prefix.into_bytes();
    data.extend(b"TEST_INIT_DATA");
    data
}

fn build_master_playlist(spec: &ResolvedHlsSpec) -> String {
    let mut playlist = String::from("#EXTM3U\n#EXT-X-VERSION:6\n");
    for (variant, bandwidth) in spec.variant_bandwidths.iter().copied().enumerate() {
        playlist.push_str(&format!(
            "#EXT-X-STREAM-INF:BANDWIDTH={bandwidth}\n{{spec}}/v{variant}.m3u8\n"
        ));
    }
    playlist
}

fn build_media_playlist(spec: &ResolvedHlsSpec, variant: usize) -> String {
    let mut playlist = format!(
        "#EXTM3U\n\
         #EXT-X-VERSION:6\n\
         #EXT-X-TARGETDURATION:{}\n\
         #EXT-X-MEDIA-SEQUENCE:0\n\
         #EXT-X-PLAYLIST-TYPE:VOD\n",
        spec.segment_duration_secs.ceil() as u64,
    );
    if spec.init_mode.is_present_for(variant) {
        playlist.push_str(&format!("#EXT-X-MAP:URI=\"init/v{variant}.mp4\"\n"));
    }
    if let Some(enc) = &spec.encryption {
        playlist.push_str("#EXT-X-KEY:METHOD=AES-128,URI=\"../key.bin\"");
        if let Some(iv) = enc.iv_hex() {
            playlist.push_str(&format!(",IV=0x{iv}"));
        }
        playlist.push('\n');
    }
    for segment in 0..spec.segments_per_variant {
        playlist.push_str(&format!(
            "#EXTINF:{:.1},\nseg/v{variant}_{segment}.m4s\n",
            spec.segment_duration_secs
        ));
    }
    playlist.push_str("#EXT-X-ENDLIST\n");
    playlist
}

impl ResolvedInitMode {
    fn is_present_for(&self, variant: usize) -> bool {
        match self {
            Self::None => false,
            Self::TestInit | Self::WavHeader { .. } => true,
            Self::PerVariantBytes(data) => data.get(variant).is_some_and(|bytes| !bytes.is_empty()),
        }
    }
}

fn derive_iv(enc: &ResolvedEncryption, sequence: usize) -> [u8; 16] {
    enc.iv.unwrap_or_else(|| {
        let mut iv = [0u8; 16];
        iv[8..16].copy_from_slice(&(sequence as u64).to_be_bytes());
        iv
    })
}

fn encrypt_aes128_cbc(data: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Vec<u8> {
    let encryptor = Encryptor::<Aes128>::new(key.into(), iv.into());
    let padded_len = data.len() + (16 - data.len() % 16);
    let mut buf = vec![0u8; padded_len];
    buf[..data.len()].copy_from_slice(data);
    let ciphertext = encryptor
        .encrypt_padded_mut::<Pkcs7>(&mut buf, data.len())
        .expect("encrypt_padded_mut");
    ciphertext.to_vec()
}

#[cfg(test)]
mod tests {
    use crate::{
        fixture_protocol::{DataMode, EncryptionRequest},
        hls_spec::parse_hls_spec_with,
        hls_url::{HlsSpec, encode_hls_spec},
    };

    use super::*;

    #[test]
    fn builds_master_and_media_playlist() {
        let spec =
            parse_hls_spec_with(&encode_hls_spec(&HlsSpec::default()), |_| unreachable!()).unwrap();
        let generated = GeneratedHls::new(spec);
        assert!(
            generated
                .master_playlist("{encoded}")
                .contains("{encoded}/v0.m3u8")
        );
        assert!(
            generated
                .media_playlist(0)
                .unwrap()
                .contains("seg/v0_0.m4s")
        );
    }

    #[test]
    fn encrypts_segment_payload() {
        let spec = parse_hls_spec_with(
            &encode_hls_spec(&HlsSpec {
                segments_per_variant: 1,
                segment_size: 32,
                data_mode: DataMode::TestPattern,
                encryption: Some(EncryptionRequest {
                    key_hex: "30313233343536373839616263646566".to_string(),
                    iv_hex: Some("00000000000000000000000000000000".to_string()),
                }),
                ..HlsSpec::default()
            }),
            |_| unreachable!(),
        )
        .unwrap();
        let generated = GeneratedHls::new(spec);
        let bytes = generated.segment_bytes(0, 0).unwrap();
        assert_ne!(bytes, generate_segment(0, 0, 32));
    }
}
