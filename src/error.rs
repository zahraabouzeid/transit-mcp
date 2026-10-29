#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct TransitError(String);

impl TransitError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

pub type Result<T> = std::result::Result<T, TransitError>;

impl From<TransitError> for String {
    fn from(error: TransitError) -> Self {
        error.0
    }
}
