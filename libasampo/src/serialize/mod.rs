// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use crate::errors::Error;

mod samples;
mod sources;

pub trait TryIntoDomain<T> {
    fn try_into_domain(self) -> Result<T, Error>;
}

pub use samples::Sample;
pub use sources::Source;
