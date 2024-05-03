// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use crate::errors::Error;

mod samples;
mod samplesets;
mod sources;

pub use samples::Sample;
pub use samplesets::SampleSet;
pub use sources::Source;

pub trait TryIntoDomain<T> {
    fn try_into_domain(self) -> Result<T, Error>;
}

pub trait TryFromDomain<T> {
    fn try_from_domain(value: &T) -> Result<Self, Error>
    where
        Self: Sized;
}

pub trait Serialize {
    fn serialize(&self) -> Result<serde_json::Value, Error>;
}

impl Serialize for crate::samples::Sample {
    fn serialize(&self) -> Result<serde_json::Value, Error> {
        serde_json::to_value(Sample::try_from_domain(self)?)
            .map_err(|e| Error::SerializationError(format!("Failed to serialize {self:?}: {e:?}")))
    }
}

impl Serialize for crate::samplesets::SampleSet {
    fn serialize(&self) -> Result<serde_json::Value, Error> {
        serde_json::to_value(SampleSet::try_from_domain(self)?)
            .map_err(|e| Error::SerializationError(format!("Failed to serialize {self:?}: {e:?}")))
    }
}

impl Serialize for crate::sources::Source {
    fn serialize(&self) -> Result<serde_json::Value, Error> {
        serde_json::to_value(Source::try_from_domain(self)?)
            .map_err(|e| Error::SerializationError(format!("Failed to serialize {self:?}: {e:?}")))
    }
}

pub fn serialize<T: Serialize>(value: &T) -> Result<serde_json::Value, Error> {
    value.serialize()
}

pub trait Deserialize {
    fn deserialize(json: serde_json::Value) -> Result<Self, Error>
    where
        Self: Sized;
}

impl Deserialize for crate::samples::Sample {
    fn deserialize(json: serde_json::Value) -> Result<Self, Error> {
        serde_json::from_value::<Sample>(json)
            .map_err(|e| Error::DeserializationError(format!("Failed to deserialize: {e:?}")))?
            .try_into_domain()
    }
}

impl Deserialize for crate::samplesets::SampleSet {
    fn deserialize(json: serde_json::Value) -> Result<Self, Error> {
        serde_json::from_value::<SampleSet>(json)
            .map_err(|e| Error::DeserializationError(format!("Failed to deserialize: {e:?}")))?
            .try_into_domain()
    }
}

impl Deserialize for crate::sources::Source {
    fn deserialize(json: serde_json::Value) -> Result<Self, Error> {
        serde_json::from_value::<Source>(json)
            .map_err(|e| Error::DeserializationError(format!("Failed to deserialize: {e:?}")))?
            .try_into_domain()
    }
}

pub fn deserialize<T: Deserialize>(json: serde_json::Value) -> Result<T, Error> {
    T::deserialize(json)
}
