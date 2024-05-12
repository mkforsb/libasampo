// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use std::{error::Error as StdError, io};

use thiserror::Error as ThisError;
use uuid::Uuid;

#[derive(ThisError, Debug)]
pub enum Error {
    #[error("I/O error for \"{uri}\": {details}")]
    IoError { uri: String, details: String },

    #[error("Symphonia error: {0}")]
    SymphoniaError(#[from] symphonia::core::errors::Error),

    #[error("Symphonia error: No default track")]
    SymphoniaNoDefaultTrackError,

    #[error("Source error: \"{uri}\" is not a valid URI for \"{source_type}\"")]
    SourceInvalidUriError { uri: String, source_type: String },

    #[error("Sample set error: sample \"{uri}\" is not present")]
    SampleSetSampleNotPresentError { uri: String },

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Deserialization error: {0}")]
    DeserializationError(String),

    #[error("Sample {0} missing source UUID")]
    SampleMissingSourceUUIDError(String),

    #[error("Missing source: {0}")]
    MissingSourceError(Uuid),
}

impl Error {
    pub fn io_error<T, U>(uri: T, details: U) -> Error
    where
        T: Into<String>,
        U: Into<String>,
    {
        Error::IoError {
            uri: uri.into(),
            details: details.into(),
        }
    }
}

impl From<glob::GlobError> for Error {
    fn from(value: glob::GlobError) -> Self {
        match value.path().to_str() {
            Some(path) => Error::IoError {
                uri: String::from(path),
                details: value.to_string(),
            },
            None => Error::IoError {
                uri: String::from("{n/a}"),
                details: value.to_string(),
            },
        }
    }
}

impl From<glob::PatternError> for Error {
    fn from(value: glob::PatternError) -> Self {
        Error::IoError {
            uri: String::from("{n/a}"),
            details: format!("Glob pattern error: {}", value),
        }
    }
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Error::IoError {
            uri: String::from("{n/a}"),
            details: value.to_string(),
        }
    }
}

pub struct LogDiscardState<T> {
    inner: T,
    level: log::Level,
}

impl<T, V, E> Iterator for LogDiscardState<T>
where
    T: Iterator<Item = Result<V, E>>,
    E: StdError,
{
    type Item = V;

    fn next(&mut self) -> Option<Self::Item> {
        match self.inner.next() {
            Some(Ok(val)) => Some(val),
            Some(Err(e)) => {
                log::log!(self.level, "{}", e);
                self.next()
            }
            None => None,
        }
    }
}

pub trait LogDiscard<T> {
    fn log_and_discard_errors(self, level: log::Level) -> LogDiscardState<T>;
}

impl<T> LogDiscard<T> for T
where
    T: Iterator,
{
    fn log_and_discard_errors(self, level: log::Level) -> LogDiscardState<T> {
        LogDiscardState { inner: self, level }
    }
}
