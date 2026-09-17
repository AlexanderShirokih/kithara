use thiserror::Error;

#[derive(Debug, Error)]
pub enum AndroidBackendError {
    #[error("android runtime was not initialized")]
    NotInitialized,

    #[error("android backend failed during {operation}: {details}")]
    Operation {
        operation: &'static str,
        details: String,
    },
}

impl AndroidBackendError {
    #[must_use]
    pub fn operation<D: Into<String>>(operation: &'static str, details: D) -> Self {
        Self::Operation {
            operation,
            details: details.into(),
        }
    }
}
