use kithara::{
    assets::AssetStore,
    bufpool::{OverallBudget, Percent, PoolConfig, PoolError, PoolRegion, pool_schema},
    play::{PlayWorker, PlaybackResamplerBackend, ResourceConfig},
    queue::{Queue, QueueControl, TrackSource},
};

use crate::consts;

pool_schema! {
    /// Buffer pools owned by one FFI engine composition root.
    pub FfiPools {
        bytes: u8,
        samples: f32,
    }
}

/// Concrete buffer-pool facade used by FFI player surfaces.
pub type Pools = PoolRegion<FfiPools>;

pub(crate) type FfiStore = AssetStore<FfiPools>;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) type FfiHost = kithara::host::Host<FfiPools>;
#[cfg(target_arch = "wasm32")]
pub(crate) type FfiHost = kithara::host::Host<FfiPools>;
pub(crate) type FfiWorker = PlayWorker<FfiPools>;
pub(crate) type FfiResourceConfig<B = PlaybackResamplerBackend> = ResourceConfig<FfiPools, B>;
pub(crate) type FfiQueue = Queue<FfiPools>;
pub(crate) type FfiQueueControl = QueueControl<FfiPools>;
pub(crate) type FfiTrackSource = TrackSource<FfiPools>;

/// Build one explicitly registered FFI pool region.
///
/// # Errors
/// Returns an error when pool configuration or initial allocation fails.
pub fn build() -> Result<Pools, PoolError> {
    FfiPools::builder(OverallBudget(consts::OVERALL_BYTES))
        .bytes(
            PoolConfig::builder()
                .initial_buffers(0)
                .max_buffers(consts::BYTE_MAX_BUFFERS)
                .max_retained_capacity(consts::BYTE_MAX_RETAINED_CAPACITY)
                .max_share(Percent::MAX)
                .build(),
        )
        .samples(
            PoolConfig::builder()
                .initial_buffers(consts::INITIAL_SAMPLE_BUFFERS)
                .initial_capacity(consts::INITIAL_SAMPLE_CAPACITY)
                .max_buffers(consts::SAMPLE_MAX_BUFFERS)
                .max_retained_capacity(consts::SAMPLE_MAX_RETAINED_CAPACITY)
                .max_share(Percent::MAX)
                .build(),
        )
        .build()
}
