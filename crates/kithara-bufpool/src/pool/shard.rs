use crossbeam_queue::ArrayQueue;

use super::storage::Storage;
use crate::PoolConfig;

pub(super) struct PoolShard<B> {
    free: ArrayQueue<B>,
}

impl<B> PoolShard<B>
where
    B: Storage,
{
    pub(super) const MAX_SLOTS: usize = 1024;

    pub(super) fn new(max_buffers: usize) -> Self {
        Self {
            free: ArrayQueue::new(max_buffers.min(Self::MAX_SLOTS)),
        }
    }

    pub(super) fn drain(&self, mut release: impl FnMut(B)) {
        while let Some(value) = self.free.pop() {
            release(value);
        }
    }

    pub(super) fn normalize(value: &mut B, config: &PoolConfig) -> bool {
        value.clear();
        value.capacity() > 0
            && (config.max_retained_capacity == 0
                || value.capacity() <= config.max_retained_capacity)
    }

    pub(super) fn try_put(&self, mut value: B, config: &PoolConfig) -> Result<(), B> {
        if !Self::normalize(&mut value, config) {
            return Err(value);
        }
        self.free.push(value)
    }

    delegate::delegate! {
        to self.free {
            #[call(pop)]
            pub(super) fn try_get(&self) -> Option<B>;
            pub(super) fn len(&self) -> usize;
        }
    }
}
