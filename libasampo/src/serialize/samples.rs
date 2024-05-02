// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{errors::Error, samples::SampleOps, serialize::TryIntoDomain};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BaseSampleV1 {
    uri: String,
    name: String,
    rate: u32,
    channels: u8,
    format: String,
    source_uuid: Option<Uuid>,
}

impl TryIntoDomain<crate::samples::Sample> for BaseSampleV1 {
    fn try_into_domain(self) -> Result<crate::samples::Sample, Error> {
        Ok(crate::samples::Sample::BaseSample(
            crate::samples::BaseSample::new(
                self.uri,
                self.name,
                crate::samples::SampleMetadata {
                    rate: self.rate,
                    channels: self.channels,
                    src_fmt_display: self.format,
                },
                self.source_uuid,
            ),
        ))
    }
}

impl TryFrom<crate::samples::BaseSample> for BaseSampleV1 {
    type Error = crate::errors::Error;

    fn try_from(value: crate::samples::BaseSample) -> Result<Self, Self::Error> {
        Ok(BaseSampleV1 {
            uri: value.uri().to_string(),
            name: value.name().to_string(),
            rate: value.metadata().rate,
            channels: value.metadata().channels,
            format: value.metadata().src_fmt_display.clone(),
            source_uuid: value.source_uuid().copied(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Sample {
    BaseSampleV1(BaseSampleV1),
}

impl TryIntoDomain<crate::samples::Sample> for Sample {
    fn try_into_domain(self) -> Result<crate::samples::Sample, Error> {
        match self {
            Self::BaseSampleV1(x) => x.try_into_domain(),
        }
    }
}

impl TryFrom<crate::samples::Sample> for Sample {
    type Error = crate::errors::Error;

    fn try_from(value: crate::samples::Sample) -> Result<Self, Self::Error> {
        match value {
            crate::samples::Sample::BaseSample(x) => {
                Ok(Sample::BaseSampleV1(BaseSampleV1::try_from(x)?))
            }
            crate::samples::Sample::DefaultSample => Err(Error::DeserializationError(
                "De/serialization not supported for DefaultSample".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basesample() {
        let uri = String::from("file:///sample.wav");
        let name = String::from("sample.wav");
        let rate = 12345;
        let channels = 7;
        let format = String::from("SuperPCM");
        let source_uuid = uuid::uuid!("10000001-2002-3003-4004-500000000005");

        let x = Sample::BaseSampleV1(BaseSampleV1 {
            uri: uri.clone(),
            name: name.clone(),
            rate,
            channels,
            format: format.clone(),
            source_uuid: Some(source_uuid),
        });

        let encoded = serde_json::to_string(&x).unwrap();
        let decoded = serde_json::from_str::<Sample>(&encoded).unwrap();

        match &decoded {
            Sample::BaseSampleV1(s) => {
                assert_eq!(s.uri, uri);
                assert_eq!(s.name, name);
                assert_eq!(s.rate, rate);
                assert_eq!(s.channels, channels);
                assert_eq!(s.format, format);
                assert_eq!(s.source_uuid, Some(source_uuid));
            }

            #[allow(unreachable_patterns)]
            _ => panic!(),
        }

        let domained = decoded.clone().try_into_domain().unwrap();

        match domained {
            crate::samples::Sample::BaseSample(s) => {
                assert_eq!(s.uri(), uri);
                assert_eq!(s.name(), name);
                assert_eq!(s.metadata().rate, rate);
                assert_eq!(s.metadata().channels, channels);
                assert_eq!(s.metadata().src_fmt_display, format);
                assert_eq!(s.source_uuid(), Some(source_uuid).as_ref());
            }

            _ => panic!(),
        }
    }
}
