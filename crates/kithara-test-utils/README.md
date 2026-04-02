<div align="center">
  <img src="../../logo.svg" alt="kithara" width="300">
</div>

<div align="center">

[![CI](https://github.com/zvuk/kithara/actions/workflows/ci.yml/badge.svg)](https://github.com/zvuk/kithara/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](../../LICENSE-MIT)

</div>

# kithara-test-utils

Shared test utilities for the kithara workspace. Provides fixtures, deterministic data generators, synthetic HLS helpers, and local test helpers used by integration tests and benchmarks.

This crate is test-only infrastructure (`publish = false`), not part of runtime playback APIs.

## Usage

```rust
use kithara_test_utils::{create_test_wav, TestHttpServer, Xorshift64};

// Deterministic audio fixture
let wav = create_test_wav(4096, 44_100, 2);

// Deterministic PRNG for stress scenarios
let mut rng = Xorshift64::new(0xDEADBEEF);
let pos = rng.range_u64(0, 10_000);

// Local HTTP fixture server for tests (native only)
let server = TestHttpServer::new(router).await;
let url = server.url("/master.m3u8");
```

## Modules

<table>
<tr><th>Module</th><th>Platform</th><th>Purpose</th></tr>
<tr><td><code>fixtures</code></td><td>cross-platform</td><td>Reusable <code>rstest</code> fixtures (<code>temp_dir</code>, cancel tokens, tracing setup)</td></tr>
<tr><td><code>fixture_protocol</code></td><td>cross-platform</td><td>Shared synthetic HLS payload types (<code>DataMode</code>, <code>InitMode</code>, <code>DelayRule</code>) and pure generation helpers (<code>generate_segment</code>, <code>expected_byte_at_test_pattern</code>)</td></tr>
<tr><td><code>hls_fixture</code></td><td>cross-platform</td><td>Canonical HLS fixture presets and config helpers backed by unified <code>/stream/*</code> routes (<code>TestServer</code>, <code>HlsTestServer</code>, <code>AbrTestServer</code>)</td></tr>
<tr><td><code>http_server</code></td><td>native only</td><td><code>TestHttpServer</code> wrapper over Axum bound to random localhost port</td></tr>
<tr><td><code>memory_source</code></td><td>cross-platform</td><td>In-memory <code>Source</code> implementations for stream/read+seek tests</td></tr>
<tr><td><code>rng</code></td><td>cross-platform</td><td>Deterministic <code>Xorshift64</code> generator for reproducible stress tests</td></tr>
<tr><td><code>wav</code></td><td>cross-platform</td><td>WAV fixture generators: <code>create_test_wav</code>, <code>create_saw_wav</code></td></tr>
<tr><td><code>test_server</code> / <code>routes::{signal,stream,token}</code></td><td>native only</td><td>Spec-driven test server routes, including procedural <code>/signal/...{wav,mp3,flac,aac,m4a}</code>, synthetic <code>/stream/*</code>, and transparent <code>POST /token</code> registration</td></tr>
</table>

## Fixture Server Protocol

The `fixture_protocol` module defines the transport-agnostic building blocks used by synthetic HLS fixtures:

- **Data modes**: `DataMode::TestPattern`, custom/blob-backed payloads, and fixture-specific synthetic presets used by legacy-compatible tests
- **Init modes**: `InitMode::None`, `InitMode::WavHeader`, custom/blob-backed init payloads
- **Delay rules**: `DelayRule` with `variant`, `segment_eq`, `segment_gte`, `delay_ms`
- **Encryption**: `EncryptionRequest` for AES-128 HLS testing
- **Data generation**: Pure functions for segment/WAV data — shared between server and client for byte-level verification

The `hls_fixture` module is the canonical test-facing API for synthetic HLS scenarios. It builds URLs against unified `test_server` routes:

- `POST /token` for registering JSON fixture specs and receiving UUID tokens
- `/stream/*` for synthetic, spec-driven HLS playlists, init segments, media segments, and keys
- `/signal/*` for procedural audio signals backed by the same token registration flow
- `/assets/*` for real regression assets stored in the repository

`TestServerHelper` hides token registration from callers. Tests still request ordinary `Url`s, while the helper first posts the JSON spec, receives a UUID, and then returns the corresponding `/signal/{token}` or `/stream/{token}` URL.

For custom synthetic HLS fixtures, prefer `TestServerHelper::create_hls(HlsFixtureBuilder::new()...)`.
That DSL keeps the server core generic while returning a typed `CreatedHls` handle with stable `master_url()`, `media_url()`, `init_url()`, `segment_url()`, and `key_url()` accessors.

## Integration

Used by the `tests/` integration crate and benchmark targets to keep fixtures centralized and deterministic. On both native and WASM, synthetic HLS now goes through the same unified `test_server` contract.

## Native Encoded Audio

The native `/signal/...` routes can render finite procedural audio as `wav`, `mp3`, `flac`, `aac`, or `m4a`.
The encoded formats use `ffmpeg-next` and therefore require a system FFmpeg installation discoverable through `pkg-config` during native builds.
