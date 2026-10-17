use super::OwnedBuffer;
use crate::PoolError;

/// A checked UTF-8 guard returned by a registered [`crate::StringKey`].
#[derive(derive_more::Debug)]
#[debug("{:?}", _0.value)]
pub struct PooledString<const SHARDS: usize>(pub(super) OwnedBuffer<SHARDS, String, true>);

impl<const SHARDS: usize> PooledString<SHARDS> {
    pub(crate) fn new(inner: OwnedBuffer<SHARDS, String, true>) -> Self {
        Self(inner)
    }

    delegate::delegate! {
        to self.0 {
            /// Return the allocated byte capacity.
            #[must_use]
            pub fn capacity(&self) -> usize;
            /// Remove all text while retaining capacity.
            pub fn clear(&mut self);
            /// Append UTF-8 text under the region budget.
            ///
            /// # Errors
            ///
            /// Returns an error when capacity overflows, exceeds the region budget,
            /// or cannot be allocated.
            pub fn try_push_str(&mut self, content: &str) -> Result<(), PoolError>;
        }
    }
}

impl<const SHARDS: usize> std::ops::Deref for PooledString<SHARDS> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0.value
    }
}
