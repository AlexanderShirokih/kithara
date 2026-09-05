# Progress

What is in flight right now. The
[GitHub Projects board](https://github.com/users/gerasim13/projects/3) owns
capability status and the roadmap, and git owns the facts. This file owns
intent: what is being worked on, what comes next, what is stuck. Update it in
the change that lands the work, and keep it short.

## In Flight

- Build and test warnings, cleared. The four `Atomic*::fetch_update` sites
  moved to the `compare_exchange_weak` loop it compiles into, keeping every
  ordering, because `loom` 0.7.2 carries only the deprecated name. MSRV is
  1.95, and `kithara-app`'s GUI-only modules are gated on `gui`.

- The `sccache` trap in the Clippy path, closed: a non-zero `CARGO_INCREMENTAL`
  makes `sccache` abort rather than fall back, for any language, and no site
  sets one now.

- Lint debt worked down by autofix. `struct_init_order`, `derivable_from` and
  `qualified_path_depth` answer to the clippy gate they used to break, the arch
  baseline drops what nothing violates, and `lint fast` runs `style`, so the
  commit hook refuses what used to reach CI.

- Configuration document for `kithara-app`: `app.yaml` plus an optional
  overlay, env-expanded before typing, each section carrying its owning
  crate's `#[derive(Patch)]` type. Open: assembly sits in `main.rs` where no
  test pins it, and twenty-two files take pools from `PoolsSection::default()`.

- Tooling parameter ownership: every policy number `xtask` and
  `kithara-devtools` held as a `const` has a config owner, and spawned programs
  resolve through `ToolsConfig`.

- Mac CI host cleanup gave the hourly pass a watchdog. Open: `deps:deny` gates
  a quarantine pipeline directly, so one network stall holds every pull
  request.

- One owner of track analysis in `kithara-app`, `AnalysisService`, and one
  extent per pass in `kithara-analysis`. Left: the deck scenario on a release
  build with the full model, and the size of the resume blob.

- `suite_network` has been dark since `#260`; the handover census found it.

- `kithara-analysis` compiles for `wasm32`. The worker's compute seam submits
  an admitted job to a spawned thread there, under one owned-pool configuration
  shared with the native Rayon backend, so beat detection has a path off the
  dispatcher thread in the browser. `just platform wasm check` covers
  `kithara-analysis` and `kithara-play`, and `just test wasm` carries the
  `kithara-worker` and `kithara-analysis` library tests; both pass in Safari,
  2 of 2 and 190 of 190. A test driving the pass itself waits on the compute
  job with an `await`: a browser runs a spawned thread only while the context
  that spawned it keeps taking events.

- The browser FFI computes track analysis. `kithara-ffi`'s wasm arm enables
  `analysis-waveform` and `beat-dsp`; `AudioPlayer.analyze(trackId)` starts a
  pass and `AudioPlayer.setAnalysisObserver(fn)` receives every publication as
  one plain object with the waveform and beat grid as copied typed arrays.
  One pass is live per track; analysing again begins a new revision sequence
  and the pass it replaces falls silent. Web warm-up creates the audio context
  at the session's own rate, and a built resource config carries the playback
  worker, so the analysis reader opens. The `/signal` route serves a 126 BPM
  click track. In Safari the pass covers all 30 s and settles in about 2.5 s;
  `web-analysis` is 1 of 2. Left: the browser detector answers with an empty
  grid, while the beat analyzer and the DSP detector are green there on
  synthetic clicks.

## Next

- 678 comment findings are decisions `--fix` cannot make.
- `kithara-ui` warns on 627 items where the widget layer compiles without a
  host, under `--features render` and `--features vello`.
- `.wasm-slim.toml` budgets wasm at 29000 KiB against a local `dist` of 3565
  KiB; the `web-size` lane on GitLab settles which number is real.
- No runtime number backs the release optimization, and the workspace's own
  crates are still at `"z"`.
- `block` 0.1.6 is a future-incompat report nothing here can answer: it reaches
  the tree through `cpal` and has no successor.

## Blocked

- Nothing.
