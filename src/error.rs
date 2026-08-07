use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Unknown read mode")]
    UnknownReadMode,

    #[error(transparent)]
    Parsing(#[from] ParsingError),
}

#[derive(Debug, Error)]
pub enum ParsingError {
    #[error("Error parsing user id")]
    ParseUserIdError,
}
