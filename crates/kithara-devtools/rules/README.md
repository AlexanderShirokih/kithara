# AST rule ownership

Only the three rows marked **shared** ship as the initial selectable pack.
Their YAML files in this directory are their single maintained source; the
pinned `kithara-devtools` dependency revision pins their IDs and contents.
Materialized scan files are disposable output, never a second source.

The other rules remain in the workspace's `.config/ast-grep/`. Classification
does not select them or promise portable semantics:

- **generic candidate**: a Rust, performance or style check without a required
  Kithara primitive. Adoption still needs its own policy and fixture validation.
- **optional primitive contract**: requires explicit adoption of Kithara's
  platform, test-harness or pool contract and its sanctioned owner sites.
- **Kithara project policy**: enforces this workspace's ownership, layout or
  implementation choices; it is not consumer policy.

Each workspace owns selection and any per-rule source globs under `[ast-grep]`
in `.config/xtask.toml`, general source scope under `[workspace-scan]`, and
test/tooling exclusions and scan-all exceptions under `[lint_exclude]`. The
shared YAML uses neutral Rust globs. Local `sgconfig.yml` composes project
rules; it does not provide fallback copies of the pack. `[tools.ast-grep]`
selects the executable and its `.config/ci-pins.toml` version key.

The inventory lists individual IDs, including every document in a multi-rule
YAML file. `tests/ast_inventory.rs` checks exact coverage, unique ownership,
delivery location and the finite three-source package.

| Rule ID | Ownership | Delivery |
| --- | --- | --- |
| `arch.jni-entry-scope` | generic candidate | local |
| `arch.no-arc-mutex-collection` | generic candidate | local |
| `arch.no-arc-mutex-godmap` | generic candidate | local |
| `arch.no-cfg-on-statement` | generic candidate | local |
| `arch.no-direct-thread-wait` | optional primitive contract | local |
| `arch.no-direct-time` | optional primitive contract | local |
| `arch.no-direct-tokio-dependency` | optional primitive contract | local |
| `arch.no-flash-time-hacks` | Kithara project policy | local |
| `arch.no-implicit-clock` | optional primitive contract | local |
| `arch.no-implicit-rng` | generic candidate | local |
| `arch.no-implicit-sleep` | optional primitive contract | local |
| `arch.no-raw-flash-toggle` | optional primitive contract | local |
| `arch.no-raw-no-block` | optional primitive contract | local |
| `arch.no-raw-test` | optional primitive contract | local |
| `arch.no-raw-tokio-sync` | optional primitive contract | local |
| `arch.no-raw-tokio-task` | optional primitive contract | local |
| `arch.no-std-sync-mutex` | optional primitive contract | local |
| `arch.no-target-os-outside-platform` | Kithara project policy | local |
| `arch.no-target-os-outside-platform.android-root` | Kithara project policy | local |
| `arch.no-target-os-outside-platform.ffi-root` | Kithara project policy | local |
| `arch.no-target-os-outside-platform.jni-root` | Kithara project policy | local |
| `arch.no-test-in-prod` | Kithara project policy | local |
| `arch.prefer-config-over-new-params` | Kithara project policy | local |
| `arch.prefer-dashmap-dashset` | Kithara project policy | local |
| `arch.test-modules-use-shared-primitives` | Kithara project policy | local |
| `arch.tests-use-pub-only` | Kithara project policy | local |
| `arch.tests-use-shared-primitives` | Kithara project policy | local |
| `idioms.derive-setter-candidate` | generic candidate | local |
| `idioms.for-if-filter-shape` | generic candidate | local |
| `idioms.join-of-join-alls` | generic candidate | local |
| `idioms.match-self-conversion` | generic candidate | local |
| `perf.no-collect-iter-roundtrip` | generic candidate | shared |
| `perf.no-component-pool-construction` | Kithara project policy | local |
| `perf.no-iter-count` | generic candidate | local |
| `perf.no-local-test-pools` | optional primitive contract | local |
| `perf.no-manual-pool-registration` | optional primitive contract | local |
| `perf.no-take-after-zip` | generic candidate | local |
| `perf.prefer-cloned` | generic candidate | local |
| `perf.prefer-primitive-pool` | optional primitive contract | local |
| `perf.reuse-layout-scratch` | Kithara project policy | local |
| `rust.no-anyhow-in-pub-api` | generic candidate | local |
| `rust.no-crate-lint-allow` | generic candidate | local |
| `rust.no-debug-prints` | generic candidate | shared |
| `rust.no-discard-result` | generic candidate | local |
| `rust.no-error-message-substring` | generic candidate | local |
| `rust.no-error-string-match` | generic candidate | local |
| `rust.no-expect-bare-string` | generic candidate | local |
| `rust.no-inherent-to-string` | generic candidate | shared |
| `rust.no-lint-suppression` | generic candidate | local |
| `rust.no-sentinel-const-alias` | generic candidate | local |
| `rust.no-sentinel-fallback` | generic candidate | local |
| `rust.no-thin-async-wrapper` | generic candidate | local |
| `rust.no-thin-sync-wrapper` | generic candidate | local |
| `rust.no-to-string-method` | generic candidate | local |
| `rust.prefer-from-iterator` | generic candidate | local |
| `rust.prefer-from-not-to-into` | generic candidate | local |
| `rust.prefer-from-trait` | generic candidate | local |
| `rust.prefer-fromstr` | generic candidate | local |
| `rust.prefer-io-write-read` | generic candidate | local |
| `rust.prefer-tryfrom-trait` | generic candidate | local |
| `style.bool-naming` | generic candidate | local |
| `style.module-consts-in-crate-consts` | Kithara project policy | local |
| `style.multiple-private-module-consts` | Kithara project policy | local |
| `style.narrow-pub-inputs` | generic candidate | local |
| `style.no-duplicate-impl` | generic candidate | local |
| `style.no-impl-only-consts` | Kithara project policy | local |
| `style.no-inline-qualified-paths` | generic candidate | local |
| `style.no-interleaved-use` | generic candidate | local |
| `style.no-items-in-lib-or-mod-rs` | generic candidate | local |
| `style.no-local-use-in-prod-functions` | generic candidate | local |
| `style.no-magic-numbers` | generic candidate | local |
| `style.no-masked-unused-arg` | generic candidate | local |
| `style.no-noop-in-impl` | generic candidate | local |
| `style.no-pub-fields-inside-crate` | generic candidate | local |
| `style.no-separator-comments-rs` | generic candidate | local |
| `style.no-separator-comments-toml` | generic candidate | local |
| `style.no-stale-file-header-doc` | generic candidate | local |
| `style.no-trivial-method` | generic candidate | local |
| `style.no-vague-type-suffix` | generic candidate | local |
| `style.prefer-any-is-none` | generic candidate | local |
| `style.prefer-any-is-some` | generic candidate | local |
| `style.prefer-default-derive` | generic candidate | local |
| `style.prefer-expect` | generic candidate | local |
| `style.pub-primitive-spread` | generic candidate | local |
