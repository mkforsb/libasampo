// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use crate::errors::Error;

mod samples;
mod samplesets;
mod sequences;
mod sources;

pub use samples::Sample;
pub use samplesets::SampleSet;
pub(crate) use samplesets::DRUMKIT_LABELS;
pub use sequences::Sequence;
pub use sources::Source;

pub trait TryIntoDomain<T> {
    fn try_into_domain(self) -> Result<T, Error>;
}

pub trait TryFromDomain<T> {
    fn try_from_domain(value: &T) -> Result<Self, Error>
    where
        Self: Sized;
}
