use thiserror::Error;

#[derive(Debug, Error)]
pub enum SilkError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("unsupported feature: {0}")]
    Unsupported(String),
}

pub type SilkResult<T> = Result<T, SilkError>;
