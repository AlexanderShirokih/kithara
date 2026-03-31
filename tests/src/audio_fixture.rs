//! Test fixtures for decode tests.
//!
//! Provides deterministic local fixtures for decode tests (no external network).
//! Includes tiny MP3/AAC test assets embedded or served by local server.

/// A tiny WAV file (0.1 seconds of silence, 44.1kHz, stereo)
/// This is a minimal valid WAV file for testing.
const TINY_WAV_BYTES: &[u8] = include_bytes!("../../assets/silence_1s.wav");

/// A test MP3 file (short audio clip)
const TEST_MP3_BYTES: &[u8] = include_bytes!("../../assets/test.mp3");

/// Embedded audio data for tests that don't need HTTP
pub struct EmbeddedAudio {
    /// WAV data (0.1 seconds of silence)
    wav: &'static [u8],
    /// MP3 data (test audio clip)
    mp3: &'static [u8],
}

impl EmbeddedAudio {
    /// Get the embedded audio data
    pub fn get() -> Self {
        Self {
            wav: TINY_WAV_BYTES,
            mp3: TEST_MP3_BYTES,
        }
    }

    /// Get WAV data
    pub fn wav(&self) -> &'static [u8] {
        self.wav
    }

    /// Get MP3 data
    pub fn mp3(&self) -> &'static [u8] {
        self.mp3
    }
}

// Native-only: AudioTestServer

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use kithara_test_utils::{SignalFormat, SignalSpec, SignalSpecLength, TestServerHelper};
    use url::Url;

    /// Test server for serving audio fixtures
    pub struct AudioTestServer {
        server: TestServerHelper,
    }

    impl AudioTestServer {
        /// Create a new test server
        pub async fn new() -> Self {
            Self {
                server: TestServerHelper::new().await,
            }
        }

        /// Get the base URL of the server
        pub fn base_url(&self) -> &str {
            self.server.base_url().as_str()
        }

        /// Get the URL for the WAV fixture
        pub fn wav_url(&self) -> Url {
            self.server.sawtooth(&wav_spec())
        }

        /// Get the URL for the MP3 fixture
        pub fn mp3_url(&self) -> Url {
            self.server.asset("test.mp3")
        }

        /// Get request count for a path
        pub fn request_count(&self, path: &str) -> usize {
            let _ = path;
            0
        }
    }

    fn wav_spec() -> SignalSpec {
        SignalSpec {
            sample_rate: 44_100,
            channels: 2,
            length: SignalSpecLength::Seconds(1.0),
            format: SignalFormat::Wav,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::AudioTestServer;

#[cfg(target_arch = "wasm32")]
mod wasm {
    use kithara_test_utils::{fixture_client, join_server_url};
    use url::Url;

    pub struct AudioTestServer {
        session_id: String,
        base_url: Url,
    }

    impl AudioTestServer {
        pub async fn new() -> Self {
            let resp = fixture_client::create_audio_fixtures_session().await;
            let mut base_url = resp.base_url;
            if !base_url.ends_with('/') {
                base_url.push('/');
            }
            Self {
                session_id: resp.session_id,
                base_url: base_url.parse().unwrap(),
            }
        }

        pub fn wav_url(&self) -> Url {
            join_server_url(&self.base_url, "audio/silence.wav")
        }

        pub fn mp3_url(&self) -> Url {
            join_server_url(&self.base_url, "audio/test.mp3")
        }

        #[allow(dead_code)]
        pub fn request_count(&self, _path: &str) -> usize {
            0 // Not tracked in fixture server
        }
    }

    impl Drop for AudioTestServer {
        fn drop(&mut self) {
            let id = self.session_id.clone();
            wasm_bindgen_futures::spawn_local(async move {
                fixture_client::delete_session(&id).await;
            });
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use wasm::AudioTestServer;
