use std::num::ParseIntError;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Unknown read mode")]
    UnknownReadMode,

    #[error(transparent)]
    Parsing(#[from] ParsingError),
}

#[derive(Debug, Error, PartialEq)]
pub enum ParsingError {
    #[error("Error parsing user id")]
    ParseUserIdError,
    #[error(transparent)]
    ParseIntError(#[from] ParseIntError),
    #[error("Got zero integer")]
    ParseNonZeroIntError,
    #[error("Got index mid-character")]
    SplitStringError,
    #[error("String either doesn't start or end on a quote")]
    ParseQuotedString,
    #[error("Error parsing a tag")]
    ParseTagError,
    #[error("Error parsing a list")]
    ParseListError,
    #[error("Line parsed successfully but left unconsumed trailing input")]
    TrailingInput,
    #[error("Capital must be at least {} usd", crate::domain::Capital::MIN)]
    CapitalTooLow,
    #[error("Liquidity must be at least {} usd", crate::domain::Liquidity::MIN)]
    LiquidityTooLow,
}
