use kithara_config::Config;
use kithara_derive::Patch;

/// Policy for one physical buffer pool in a region.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Patch, Config)]
#[config(fields(value))]
pub struct PoolConfig {
    /// Number of reusable payloads allocated during region construction.
    #[config(builder(default))]
    pub(crate) initial_buffers: usize,
    /// Element capacity of each initially allocated payload.
    #[config(builder(default))]
    pub(crate) initial_capacity: usize,
    /// Maximum number of retained buffers across all shards.
    pub(crate) max_buffers: usize,
    /// Drop returned buffers above this capacity. Zero disables the ceiling.
    #[config(builder(default))]
    pub(crate) max_retained_capacity: usize,
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use kithara_test_utils::kithara;
    use serde::Deserialize;

    use super::{PoolConfig, PoolConfigPatch};

    #[kithara::test(native, flash(false))]
    fn a_patch_writes_only_the_field_it_names() {
        let mut config = PoolConfig::builder()
            .max_buffers(8)
            .max_retained_capacity(4_096)
            .build();

        let patch = PoolConfigPatch::deserialize(serde_yaml_ng::Deserializer::from_str(
            "max_buffers: 32\n",
        ))
        .expect("a valid patch document parses");
        config.apply(patch);

        assert_eq!(config.max_buffers, 32, "the named field is written");
        assert_eq!(
            config.max_retained_capacity, 4_096,
            "a field the document does not name keeps the value it already had"
        );
    }
}
