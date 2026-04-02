//! Integration tests for audio fixtures.
//!
//! Tests that verify the audio fixtures work correctly and can be used
//! by decode tests without external network access.

use std::io::Cursor;

use kithara::decode::{DecoderConfig, DecoderFactory};
use kithara_integration_tests::audio_fixture::EmbeddedAudio;
use kithara_platform::time::Duration;
use kithara_test_utils::{
    HlsFixtureBuilder, SignalFormat, SignalSpec, SignalSpecLength, TestServerHelper,
    fixture_protocol::{DataMode, InitMode},
};
use reqwest::Client;

#[kithara::test(
    tokio,
    timeout(Duration::from_secs(5)),
    env(KITHARA_HANG_TIMEOUT_SECS = "1")
)]
async fn test_test_server_helper_serves_audio_fixture_urls() {
    let server = TestServerHelper::new().await;

    let wav_url = server.sawtooth(&wav_spec()).await;
    let mp3_url = server.asset("test.mp3");

    assert!(wav_url.as_str().starts_with("http://127.0.0.1:"));
    assert!(mp3_url.as_str().starts_with("http://127.0.0.1:"));
    assert!(wav_url.path().starts_with("/signal/sawtooth/"));
    assert!(wav_url.path().ends_with(".wav"));
    assert!(mp3_url.path().ends_with("test.mp3"));
}

#[kithara::test(
    native,
    tokio,
    timeout(Duration::from_secs(5)),
    env(KITHARA_HANG_TIMEOUT_SECS = "1")
)]
#[case("wav", "audio/wav", "WAV file")]
#[case("mp3", "audio/mpeg", "MP3 file")]
async fn test_test_server_helper_serves_format(
    #[case] format: &str,
    #[case] content_type: &str,
    #[case] desc: &str,
) {
    let server = TestServerHelper::new().await;
    let client = Client::new();

    let url = match format {
        "wav" => server.sawtooth(&wav_spec()).await,
        "mp3" => server.asset("test.mp3"),
        _ => panic!("Unknown format: {}", format),
    };

    let response = client
        .get(url)
        .send()
        .await
        .unwrap_or_else(|e| panic!("Failed to fetch {}: {}", desc, e));

    assert_eq!(response.status(), 200, "{}: status", desc);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        content_type,
        "{}: content-type",
        desc
    );

    let content_length: usize = response
        .headers()
        .get("content-length")
        .unwrap()
        .to_str()
        .unwrap()
        .parse()
        .unwrap();

    assert!(content_length > 0, "{}: content length should be > 0", desc);
}

#[kithara::test(
    native,
    tokio,
    timeout(Duration::from_secs(10)),
    env(KITHARA_HANG_TIMEOUT_SECS = "1")
)]
#[case(SignalFormat::Mp3, "mp3", "audio/mpeg")]
#[case(SignalFormat::Flac, "flac", "audio/flac")]
#[case(SignalFormat::Aac, "aac", "audio/aac")]
#[case(SignalFormat::M4a, "m4a", "audio/mp4")]
async fn test_signal_server_encoded_formats_are_decodable(
    #[case] format: SignalFormat,
    #[case] ext: &str,
    #[case] content_type: &str,
) {
    let server = TestServerHelper::new().await;
    let client = Client::new();
    let spec = SignalSpec {
        sample_rate: 44_100,
        channels: 2,
        length: SignalSpecLength::Seconds(1.0),
        format,
    };

    let response = client
        .get(server.sawtooth(&spec).await)
        .send()
        .await
        .unwrap_or_else(|error| panic!("Failed to fetch /signal encoded fixture: {error}"));

    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        content_type
    );

    let bytes = response.bytes().await.unwrap();
    assert!(!bytes.is_empty());

    let mut decoder = DecoderFactory::create_with_probe(
        Cursor::new(bytes.to_vec()),
        Some(ext),
        DecoderConfig::default(),
    )
    .unwrap();

    let chunk = decoder.next_chunk().unwrap().unwrap();
    assert!(!chunk.pcm.is_empty());
}

#[kithara::test(
    native,
    tokio,
    timeout(Duration::from_secs(5)),
    env(KITHARA_HANG_TIMEOUT_SECS = "1")
)]
async fn test_create_hls_returns_stable_typed_urls() {
    let server = TestServerHelper::new().await;
    let created = server
        .create_hls(
            HlsFixtureBuilder::new()
                .variant_count(1)
                .segments_per_variant(4)
                .segment_size(200_000)
                .segment_duration_secs(200_000.0 / (44_100.0 * 2.0 * 2.0))
                .data_mode(DataMode::SawWav {
                    sample_rate: 44_100,
                    channels: 2,
                })
                .init_mode(InitMode::WavHeader {
                    sample_rate: 44_100,
                    channels: 2,
                }),
        )
        .await
        .expect("create HLS fixture");

    let master_url = created.master_url();
    let media_url = created.media_url(0);
    let segment_url = created.segment_url(0, 0);
    let token = created.token().to_string();

    assert!(master_url.path().contains(&token));
    assert!(media_url.path().contains(&token));
    assert!(segment_url.path().contains(&token));

    let client = Client::new();
    let master = client.get(master_url).send().await.unwrap();
    assert_eq!(master.status(), 200);
    assert_eq!(
        master.headers().get("content-type").unwrap(),
        "application/vnd.apple.mpegurl"
    );
}

#[kithara::test]
fn test_embedded_audio_contains_data() {
    let audio = EmbeddedAudio::get();

    // Verify WAV data exists
    let wav_data = audio.wav();
    assert!(!wav_data.is_empty());

    // Verify MP3 data exists
    let mp3_data = audio.mp3();
    assert!(!mp3_data.is_empty());

    // MP3 should be larger than WAV (our test MP3 is 2.9MB)
    assert!(mp3_data.len() > wav_data.len());
}

fn wav_spec() -> SignalSpec {
    SignalSpec {
        sample_rate: 44_100,
        channels: 2,
        length: SignalSpecLength::Seconds(1.0),
        format: SignalFormat::Wav,
    }
}

// Note: More comprehensive decode tests will be added when the actual
// decode functionality is implemented. These tests just verify the
// fixture infrastructure works correctly.
