#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashMap;

#[cfg(not(target_arch = "wasm32"))]
use kithara::{events::EventBus, platform::CancelToken};
use kithara::{
    events::TrackId,
    platform::sync::{Arc, Mutex},
};
use uuid::Uuid;

#[cfg(not(target_arch = "wasm32"))]
use crate::native::item_bridge::ItemEventBridge;
#[cfg(not(target_arch = "wasm32"))]
use crate::types::FfiAbrMode;
use crate::{
    core::observer_set::ObserverSet,
    observer::{ItemLoadCallback, ItemObserver},
    types::{FfiItemConfig, FfiItemLoadResult, FfiItemState, FfiItemStatus, FfiTimeRange},
};

/// Loading lifecycle of an item. A sum type so the contradictory
/// boolean combinations the old packed struct allowed
/// (`ready && failed`, `ready && duration == 0`, `failed && duration`)
/// are unrepresentable: a duration only exists inside `Ready`, and
/// `Ready` / `Failed` are mutually exclusive.
#[derive(Debug, Clone, Copy)]
enum LoadingState {
    /// Inserted but no duration resolved yet (and not failed).
    Pending,
    /// Metadata resolved; `duration_sec` is the playable duration.
    Ready { duration_sec: f64 },
    /// Terminal failure — sticky, carries no duration.
    Failed,
}

/// Cached subset of item state surfaced through synchronous getters
/// (`duration_sec`, `is_live_stream`, …) and the `load()` resolver.
/// Updated by [`ItemEventBridge`] through the typed transition methods
/// as the underlying resource emits metadata events.
#[derive(Debug, Clone)]
pub(crate) struct ItemView {
    loading: LoadingState,
    has_protected_content: bool,
    is_live_stream: bool,
    error: Option<String>,
    loaded_ranges: Vec<FfiTimeRange>,
}

impl ItemView {
    const fn new(is_live_stream: bool) -> Self {
        Self {
            is_live_stream,
            loading: LoadingState::Pending,
            has_protected_content: false,
            error: None,
            loaded_ranges: Vec::new(),
        }
    }

    /// Resolved duration in seconds, or `0.0` when not yet `Ready`
    /// (pending or failed).
    const fn duration_sec(&self) -> f64 {
        match self.loading {
            LoadingState::Ready { duration_sec } => duration_sec,
            LoadingState::Pending | LoadingState::Failed => 0.0,
        }
    }

    /// Whether the item resolved metadata and is playable. False while
    /// pending or after a failure — `Ready` and `Failed` are exclusive,
    /// so this is the typed replacement for `is_ready_to_play && !is_failed`.
    const fn is_ready(&self) -> bool {
        matches!(self.loading, LoadingState::Ready { .. })
    }

    /// Terminal failure transition from any state. Reports whether this call
    /// performed it, so the second source of a terminal event sees `false` and
    /// stays silent instead of repeating the status/error pair.
    pub(crate) fn mark_failed(&mut self, reason: String) -> bool {
        if matches!(self.loading, LoadingState::Failed) {
            return false;
        }
        self.loading = LoadingState::Failed;
        self.error = Some(reason);
        true
    }

    /// Playable without a resolved duration yet — the queue reports a track
    /// loaded before the metadata layer answers.
    pub(crate) const fn mark_ready(&mut self) {
        if matches!(self.loading, LoadingState::Pending) {
            self.loading = LoadingState::Ready { duration_sec: 0.0 };
        }
    }

    pub(crate) fn replace_loaded_ranges(&mut self, ranges: Vec<FfiTimeRange>) {
        self.loaded_ranges = ranges;
    }

    fn status(&self) -> FfiItemStatus {
        match self.loading {
            LoadingState::Pending => FfiItemStatus::Unknown,
            LoadingState::Ready { .. } => FfiItemStatus::ReadyToPlay,
            LoadingState::Failed => FfiItemStatus::Failed,
        }
    }

    /// Metadata resolved with `duration_sec`. A no-op once `Failed`
    /// (failure is sticky), mirroring the old `is_failed` flag never
    /// being cleared.
    pub(crate) const fn resolve_duration(&mut self, duration_sec: f64) {
        if !matches!(self.loading, LoadingState::Failed) {
            self.loading = LoadingState::Ready { duration_sec };
        }
    }
}

/// FFI-facing audio player item.
///
/// Carries two identifiers, per iOS `AudioPlayerItemProtocol`:
/// - [`Self::audio_id`] — caller-facing content id. When
///   [`FfiItemConfig::audio_id`] is absent it falls back to the
///   internally allocated queue id for standalone Kithara callers.
/// - [`Self::uuid_i64`] — caller-facing queue-item id. When
///   [`FfiItemConfig::uuid_i64`] is absent it falls back to the
///   legacy UUIDv5-derived handle.
#[cfg_attr(feature = "uniffi", derive(uniffi::Object))]
#[derive(fieldwork::Fieldwork)]
#[fieldwork(opt_in, get)]
pub struct AudioPlayerItem {
    pub(crate) state: Arc<Mutex<ItemView>>,
    /// Scoped event bus — set by `AudioPlayer::insert` so per-resource
    /// events (Hls/File/Audio) published during `Resource::new` are
    /// captured even when [`Self::add_observer`] is called later. Native-only:
    /// the wasm worker owns the queue and its event bus.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) bus: Mutex<Option<EventBus>>,
    /// Inserted-into-queue flag — flipped by `AudioPlayer::insert` so
    /// [`Self::load`] can tell "still detached" from "loaded enough to
    /// answer playable". Pre-insert / post-remove value is `false`.
    pub(crate) inserted: Mutex<bool>,
    config: FfiItemConfig,
    /// Per-item event bridge translating resource events into
    /// [`ItemObserver`] callbacks. Native-only: the wasm worker routes
    /// item events through the main-thread event router instead (Wave 5).
    #[cfg(not(target_arch = "wasm32"))]
    event_bridge: Mutex<Option<ItemEventBridge>>,
    observers: Arc<ObserverSet>,
    /// Process-wide monotonic id allocated at construction and consumed
    /// by the core queue. Not exposed by the high-level Swift API: it is
    /// only the routing key that lets repeated business tracks coexist.
    #[field(get(
        name = track_id,
        copy,
        vis = "pub(crate)",
        doc = "Returns the strongly typed queue routing id."
    ))]
    queue_id: TrackId,
    /// Caller-facing content id returned by [`Self::audio_id`].
    audio_id: TrackId,
    /// Caller-facing queue-item uuid returned by [`Self::uuid_i64`].
    uuid_i64: i64,
}

/// Methods exported across the FFI boundary.
#[cfg_attr(feature = "uniffi", uniffi::export)]
impl AudioPlayerItem {
    /// Create a new item with frozen preferences. Reserves a fresh
    /// private queue id from the process-wide counter. Caller-supplied
    /// `audioId` / `uuid` are stored on the item and surfaced through
    /// the iOS-compatible accessors without becoming the core queue key.
    /// Loading starts automatically when the item is inserted into an
    /// [`crate::player::AudioPlayer`].
    #[must_use]
    #[cfg_attr(feature = "uniffi", uniffi::constructor)]
    pub fn new(config: FfiItemConfig) -> Arc<Self> {
        let live = config.is_live_stream;
        let queue_id = TrackId::allocate();
        let audio_id = config.audio_id.unwrap_or(queue_id);
        let uuid_i64 = config
            .uuid_i64
            .unwrap_or_else(|| derived_uuid_i64(&config.url, queue_id));
        Arc::new(Self {
            config,
            queue_id,
            audio_id,
            uuid_i64,
            #[cfg(not(target_arch = "wasm32"))]
            event_bridge: Mutex::default(),
            observers: Arc::default(),
            #[cfg(not(target_arch = "wasm32"))]
            bus: Mutex::default(),
            inserted: Mutex::default(),
            state: Arc::new(Mutex::new(ItemView::new(live))),
        })
    }

    /// Caller-facing content id. Mirrors the iOS
    /// `AudioPlayerItemProtocol.audioId: TrackId`.
    pub const fn audio_id(&self) -> TrackId {
        self.audio_id
    }

    /// Cached item duration in seconds. Defaults to `0.0` until the
    /// underlying resource emits a duration update.
    pub fn duration_sec(&self) -> f64 {
        self.state.lock().duration_sec()
    }

    /// Consistent snapshot of status, duration, failure reason and
    /// buffered ranges.
    pub fn state(&self) -> FfiItemState {
        let view = self.state.lock();
        FfiItemState {
            status: view.status(),
            duration_seconds: view.duration_sec(),
            error: view.error.clone(),
            loaded_ranges: view.loaded_ranges.clone(),
        }
    }

    /// Whether this item represents a live HLS feed. The flag is set
    /// from [`FfiItemConfig::is_live_stream`] at construction; in the
    /// future this getter will also surface auto-detected live streams.
    pub fn is_live_stream(&self) -> bool {
        self.state.lock().is_live_stream
    }

    /// Whether the item is playable at `progress` (seconds) given the
    /// caller-supplied buffered `ranges`. Live streams are reported
    /// playable unconditionally.
    #[cfg_attr(
        all(),
        expect(
            clippy::needless_pass_by_value,
            reason = "UniFFI Lift requires owned Vec across FFI ABI"
        )
    )]
    pub fn is_playable(&self, progress: f64, ranges: Vec<FfiTimeRange>) -> bool {
        if self.is_live_stream() {
            return true;
        }
        ranges
            .iter()
            .any(|r| progress >= r.start_seconds && progress < r.start_seconds + r.duration_seconds)
    }

    /// Resolve `callback` with the item's current load status. If the
    /// item has not yet been inserted into a queue (or has been
    /// removed), the callback fires with
    /// `FfiItemLoadResult { has_protected_content: false, is_playable: false }`.
    ///
    /// `load` does not trigger an additional fetch — `AudioPlayer::insert`
    /// already kicks off background loading. This method is the FFI
    /// answer to the iOS protocol's `func load() -> Observable<…>`:
    /// it surfaces the cached state once the metadata layer has caught
    /// up.
    #[cfg_attr(
        all(),
        expect(
            clippy::needless_pass_by_value,
            reason = "UniFFI Lift trait requires owned Arc — FFI ABI contract"
        )
    )]
    pub fn load(&self, callback: Arc<dyn ItemLoadCallback>) {
        let inserted = *self.inserted.lock();
        let snapshot = self.state.lock().clone();
        let result = if inserted {
            FfiItemLoadResult {
                has_protected_content: snapshot.has_protected_content,
                is_playable: snapshot.is_ready(),
            }
        } else {
            FfiItemLoadResult {
                has_protected_content: false,
                is_playable: false,
            }
        };
        callback.on_complete(result);
    }

    pub const fn preferred_peak_bitrate(&self) -> f64 {
        self.config.preferred_peak_bitrate
    }

    pub const fn preferred_peak_bitrate_for_expensive_networks(&self) -> f64 {
        self.config.preferred_peak_bitrate_expensive
    }

    /// Private queue id used by player-level events to route back to the
    /// Swift-owned item instance. High-level Swift maps it back to
    /// [`Self::audio_id`] before publishing public events.
    pub const fn queue_id(&self) -> TrackId {
        self.queue_id
    }

    /// Subscribes `observer` to this item's events and returns the handle
    /// that [`Self::remove_observer`] unsubscribes it with. Every registered
    /// observer receives every event.
    pub fn add_observer(&self, observer: Arc<dyn ItemObserver>) -> u64 {
        #[cfg(target_arch = "wasm32")]
        self.prime(&observer);
        self.observers.add(observer)
    }

    /// Unsubscribes the observer registered under `id`.
    pub fn remove_observer(&self, id: u64) {
        self.observers.remove(id);
    }

    /// Audio source string — either a network URL or an absolute local
    /// path, as supplied via [`FfiItemConfig::url`]. The Swift wrapper
    /// surfaces this as a `URL` (`file://…` for local paths) so the iOS
    /// `AudioPlayerItemProtocol.url` contract holds for both cases.
    pub fn url(&self) -> String {
        self.config.url.clone()
    }

    /// Caller-facing queue-item uuid. Maps to
    /// `AudioPlayerItemProtocol.uuid: Int64` on iOS.
    pub const fn uuid_i64(&self) -> i64 {
        self.uuid_i64
    }
}

/// Internal methods not exported across FFI.
impl AudioPlayerItem {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) const fn abr_mode(&self) -> Option<FfiAbrMode> {
        self.config.abr_mode
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn headers(&self) -> Option<HashMap<String, String>> {
        self.config.headers.clone()
    }

    pub(crate) fn observer(&self) -> Arc<dyn ItemObserver> {
        Arc::clone(&self.observers) as Arc<dyn ItemObserver>
    }

    /// (Re)subscribe the bridge to the currently-attached scoped bus.
    /// Called from `AudioPlayer::insert` right after the bus is attached.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn restart_bridge(&self) {
        let Some(bus) = self.bus.lock().clone() else {
            *self.event_bridge.lock() = None;
            return;
        };
        let bridge = ItemEventBridge::spawn(
            bus.subscribe(),
            self.observer(),
            None,
            Arc::clone(&self.state),
            CancelToken::never(),
        );
        *self.event_bridge.lock() = Some(bridge);
    }

    /// Wasm has no long-lived per-item bus bridge — the worker owns the
    /// queue and routes events through the main-thread router
    /// ([`crate::web::observer::router`]). What restart still needs to do
    /// is replay the item's cached [`ItemView`] to its observers, so they
    /// see the same initial event (`StatusChanged`) the native path emits
    /// when its bridge spawns.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn restart_bridge(&self) {
        self.prime(&self.observer());
    }

    #[cfg(target_arch = "wasm32")]
    fn prime(&self, observer: &Arc<dyn ItemObserver>) {
        let snapshot = self.state.lock().clone();
        match snapshot.loading {
            LoadingState::Failed => {
                observer.on_event(crate::types::FfiItemEvent::StatusChanged {
                    status: crate::types::FfiItemStatus::Failed,
                });
            }
            LoadingState::Ready { .. } => {
                observer.on_event(crate::types::FfiItemEvent::StatusChanged {
                    status: crate::types::FfiItemStatus::ReadyToPlay,
                });
            }
            LoadingState::Pending => {}
        }
        let duration = snapshot.duration_sec();
        if duration > 0.0 {
            observer.on_event(crate::types::FfiItemEvent::DurationChanged { seconds: duration });
        }
    }
}

fn derived_uuid_i64(url: &str, queue_id: TrackId) -> i64 {
    let key = format!("{}:{}", url, queue_id.as_u64());
    let uuid = Uuid::new_v5(&Uuid::NAMESPACE_URL, key.as_bytes());
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&uuid.as_bytes()[0..8]);
    i64::from_be_bytes(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FfiItemEvent;

    fn config_with_url(url: String) -> FfiItemConfig {
        FfiItemConfig {
            url,
            headers: None,
            audio_id: None,
            uuid_i64: None,
            preferred_peak_bitrate: 0.0,
            preferred_peak_bitrate_expensive: 0.0,
            abr_mode: None,
            is_live_stream: false,
        }
    }

    fn item_for(url: &str) -> Arc<AudioPlayerItem> {
        AudioPlayerItem::new(config_with_url(url.to_string()))
    }

    #[derive(Default)]
    struct CountingObserver {
        seen: Mutex<Vec<String>>,
    }

    impl CountingObserver {
        fn seen(&self) -> Vec<String> {
            self.seen.lock().clone()
        }
    }

    impl ItemObserver for CountingObserver {
        fn on_event(&self, event: FfiItemEvent) {
            self.seen.lock().push(format!("{event:?}"));
        }
    }

    #[kithara::test]
    fn every_registered_observer_sees_the_event_until_it_unsubscribes() {
        let item = item_for("https://example.com/a.mp3");
        let first = Arc::new(CountingObserver::default());
        let second = Arc::new(CountingObserver::default());
        let first_id = item.add_observer(Arc::clone(&first) as Arc<dyn ItemObserver>);
        item.add_observer(Arc::clone(&second) as Arc<dyn ItemObserver>);

        item.observer()
            .on_event(FfiItemEvent::DurationChanged { seconds: 1.0 });
        assert_eq!(first.seen().len(), 1);
        assert_eq!(second.seen().len(), 1);

        item.remove_observer(first_id);
        item.observer()
            .on_event(FfiItemEvent::DurationChanged { seconds: 2.0 });
        assert_eq!(first.seen().len(), 1);
        assert_eq!(second.seen().len(), 2);
    }

    #[kithara::test]
    fn audio_id_is_monotonic_across_new_items() {
        let a = item_for("https://example.com/a.mp3");
        let b = item_for("https://example.com/b.mp3");
        assert!(a.audio_id() < b.audio_id());
    }

    #[kithara::test]
    fn audio_id_is_distinct_for_two_items_of_same_url() {
        let a = item_for("https://example.com/track.mp3");
        let b = item_for("https://example.com/track.mp3");
        assert_ne!(a.audio_id(), b.audio_id());
    }

    #[kithara::test]
    fn uuid_is_distinct_for_two_items_of_same_url() {
        let a = item_for("https://example.com/track.mp3");
        let b = item_for("https://example.com/track.mp3");
        assert_ne!(a.uuid_i64(), b.uuid_i64());
    }

    #[kithara::test]
    fn uuid_i64_matches_uuid_v5_of_url_and_audio_id() {
        let url = "https://example.com/song.mp3";
        let item = item_for(url);
        let key = format!("{}:{}", url, item.track_id());
        let expected = Uuid::new_v5(&Uuid::NAMESPACE_URL, key.as_bytes());
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&expected.as_bytes()[0..8]);
        assert_eq!(item.uuid_i64(), i64::from_be_bytes(buf));
    }

    #[kithara::test]
    fn caller_audio_id_is_exposed_without_becoming_queue_id() {
        let config = FfiItemConfig {
            audio_id: Some(TrackId(42)),
            ..config_with_url("https://example.com/song.mp3".to_string())
        };
        let item = AudioPlayerItem::new(config);
        assert_eq!(item.audio_id(), TrackId(42));
        assert_ne!(item.track_id(), TrackId(42));
    }

    #[kithara::test]
    fn caller_uuid_i64_is_exposed() {
        let config = FfiItemConfig {
            uuid_i64: Some(123_456),
            ..config_with_url("https://example.com/song.mp3".to_string())
        };
        let item = AudioPlayerItem::new(config);
        assert_eq!(item.uuid_i64(), 123_456);
    }

    #[kithara::test]
    fn audio_id_and_uuid_work_for_local_path() {
        let path = "/Users/me/Music/song.flac";
        let a = item_for(path);
        let b = item_for(path);
        assert_ne!(a.audio_id(), b.audio_id());
        assert_ne!(a.uuid_i64(), b.uuid_i64());
    }

    #[kithara::test]
    fn url_preserved() {
        let item = item_for("https://example.com/song.mp3");
        assert_eq!(item.url(), "https://example.com/song.mp3");
    }

    #[kithara::test]
    fn preferred_peak_bitrate_from_config() {
        let config = FfiItemConfig {
            preferred_peak_bitrate: 256_000.0,
            ..config_with_url("https://example.com/a.mp3".to_string())
        };
        let item = AudioPlayerItem::new(config);
        assert_eq!(item.preferred_peak_bitrate(), 256_000.0);
    }

    #[kithara::test]
    fn inserted_flag_initially_false() {
        let item = item_for("https://example.com/a.mp3");
        assert!(!*item.inserted.lock());
    }

    #[kithara::test]
    fn headers_roundtrip() {
        let mut headers = HashMap::new();
        headers.insert("Authorization".into(), "Bearer token".into());
        let config = FfiItemConfig {
            headers: Some(headers),
            ..config_with_url("https://example.com/a.mp3".to_string())
        };
        let item = AudioPlayerItem::new(config);
        let returned = item
            .headers()
            .expect("BUG: headers were just set on the config above");
        assert_eq!(returned.get("Authorization"), Some(&"Bearer token".into()));
    }

    #[kithara::test]
    fn uuid_i64_is_stable_for_same_audio_id() {
        let item = item_for("https://example.com/a.mp3");
        let first = item.uuid_i64();
        let second = item.uuid_i64();
        assert_eq!(first, second);
    }

    #[kithara::test]
    fn is_live_stream_defaults_false() {
        let item = item_for("https://example.com/song.mp3");
        assert!(!item.is_live_stream());
    }

    #[kithara::test]
    fn is_live_stream_from_config() {
        let config = FfiItemConfig {
            is_live_stream: true,
            ..config_with_url("https://example.com/live.m3u8".to_string())
        };
        let item = AudioPlayerItem::new(config);
        assert!(item.is_live_stream());
    }

    #[kithara::test]
    fn is_playable_live_stream_always_true() {
        let config = FfiItemConfig {
            is_live_stream: true,
            ..config_with_url("https://example.com/live.m3u8".to_string())
        };
        let item = AudioPlayerItem::new(config);
        assert!(item.is_playable(0.0, vec![]));
        assert!(item.is_playable(9999.0, vec![]));
    }

    #[kithara::test]
    fn item_view_pending_is_not_ready_and_zero_duration() {
        let view = ItemView::new(false);
        assert!(!view.is_ready());
        assert_eq!(view.duration_sec(), 0.0);
    }

    #[kithara::test]
    fn item_view_resolve_duration_sets_ready() {
        let mut view = ItemView::new(false);
        view.resolve_duration(42.0);
        assert!(view.is_ready());
        assert_eq!(view.duration_sec(), 42.0);
    }

    #[kithara::test]
    fn item_view_mark_failed_is_not_ready_and_zero_duration() {
        let mut view = ItemView::new(false);
        view.resolve_duration(42.0);
        view.mark_failed("test failure".to_owned());
        assert!(!view.is_ready());
        assert_eq!(view.duration_sec(), 0.0);
    }

    /// Two independent sources settle a failed item — the protocol bridge and
    /// the queue — and each asks the view whether the pair is still its to
    /// emit. Only the transition itself may answer yes.
    #[kithara::test]
    fn item_view_mark_failed_reports_only_the_first_transition() {
        let mut view = ItemView::new(false);
        assert!(view.mark_failed("test failure".to_owned()));
        assert!(!view.mark_failed("test failure".to_owned()));
    }

    #[kithara::test]
    fn item_view_failure_is_sticky_over_resolve_duration() {
        let mut view = ItemView::new(false);
        view.mark_failed("test failure".to_owned());
        view.resolve_duration(42.0);
        assert!(
            !view.is_ready(),
            "resolve_duration must not un-fail a Failed item"
        );
        assert_eq!(view.duration_sec(), 0.0);
    }

    #[kithara::test]
    fn item_view_live_flag_preserved_across_transitions() {
        let mut view = ItemView::new(true);
        assert!(view.is_live_stream);
        view.resolve_duration(10.0);
        assert!(view.is_live_stream);
        view.mark_failed("test failure".to_owned());
        assert!(view.is_live_stream);
    }

    #[kithara::test]
    fn is_playable_within_ranges() {
        let item = item_for("https://example.com/song.mp3");
        let ranges = vec![FfiTimeRange {
            start_seconds: 0.0,
            duration_seconds: 30.0,
        }];
        assert!(item.is_playable(0.0, ranges.clone()));
        assert!(item.is_playable(15.0, ranges.clone()));
        assert!(!item.is_playable(30.0, ranges.clone()));
        assert!(!item.is_playable(45.0, ranges));
    }
}
