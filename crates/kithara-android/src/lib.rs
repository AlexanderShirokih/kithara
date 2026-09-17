//! Android platform ABI and safe wrappers shared by Kithara crates.
//!
//! The crate owns access to the host runtime handle: it publishes the `JavaVM`
//! and the application context, and attaches calling threads to them. Decode
//! and media policy live in `kithara-decode`, the `Java_*` entry points in
//! `kithara-ffi`.

mod error;
mod runtime;

pub use error::AndroidBackendError;
pub use runtime::{attach_current_thread, initialize};
