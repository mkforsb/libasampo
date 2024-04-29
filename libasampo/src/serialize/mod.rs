// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use samples::SampleTrait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::errors::Error;
use crate::sources::file_system_source as fs_source;
use crate::{samples, sources};

pub trait TryIntoDomain<T> {
    fn try_into_domain(self) -> Result<T, Error>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseSampleV1 {
    uri: String,
    name: String,
    rate: u32,
    channels: u8,
    format: String,
    source_uuid: Option<Uuid>,
}

impl TryIntoDomain<samples::Sample> for BaseSampleV1 {
    fn try_into_domain(self) -> Result<samples::Sample, Error> {
        Ok(samples::Sample::BaseSample(samples::BaseSample::new(
            self.uri,
            self.name,
            samples::SampleMetadata {
                rate: self.rate,
                channels: self.channels,
                src_fmt_display: self.format,
            },
            self.source_uuid,
        )))
    }
}

impl TryFrom<samples::BaseSample> for BaseSampleV1 {
    type Error = crate::errors::Error;

    fn try_from(value: samples::BaseSample) -> Result<Self, Self::Error> {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Sample {
    BaseSampleV1(BaseSampleV1),
}

impl TryIntoDomain<samples::Sample> for Sample {
    fn try_into_domain(self) -> Result<samples::Sample, Error> {
        match self {
            Self::BaseSampleV1(x) => x.try_into_domain(),
        }
    }
}

impl TryFrom<samples::Sample> for Sample {
    type Error = crate::errors::Error;

    fn try_from(value: samples::Sample) -> Result<Self, Self::Error> {
        match value {
            samples::Sample::BaseSample(x) => Ok(Sample::BaseSampleV1(BaseSampleV1::try_from(x)?)),
            samples::Sample::DefaultSample => Err(Error::DeserializationError(
                "De/serialization not supported for DefaultSample".to_string(),
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemSourceV1 {
    name: Option<String>,
    uuid: Uuid,
    path: String,
    uri: String,
    exts: Vec<String>,
    enabled: bool,
}

impl TryIntoDomain<sources::Source> for FilesystemSourceV1 {
    fn try_into_domain(self) -> Result<sources::Source, Error> {
        let mut src = fs_source::FilesystemSource::new_with_io(
            self.name,
            self.path,
            self.exts,
            fs_source::io::DefaultIO(),
        );
        src.set_uuid(self.uuid);
        Ok(sources::Source::FilesystemSource(src))
    }
}

impl<T: fs_source::io::IO> TryFrom<fs_source::FilesystemSource<T>> for FilesystemSourceV1 {
    type Error = crate::errors::Error;

    fn try_from(src: fs_source::FilesystemSource<T>) -> Result<Self, Self::Error> {
        Ok(FilesystemSourceV1 {
            name: src.name.clone(),
            uuid: src.uuid,
            path: src.path.clone(),
            uri: src.uri.clone(),
            exts: src.exts.clone(),
            enabled: src.enabled,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Source {
    FilesystemSourceV1(FilesystemSourceV1),
}

impl TryIntoDomain<sources::Source> for Source {
    fn try_into_domain(self) -> Result<sources::Source, Error> {
        match self {
            Source::FilesystemSourceV1(src) => src.try_into_domain(),
        }
    }
}

impl TryFrom<sources::Source> for Source {
    type Error = crate::errors::Error;

    fn try_from(value: sources::Source) -> Result<Self, Error> {
        match value {
            sources::Source::FilesystemSource(src) => {
                Ok(Source::FilesystemSourceV1(FilesystemSourceV1::try_from(src)?))
            }

            #[cfg(feature = "mocks")]
            sources::Source::MockSource(_) => Err(Error::SerializationError("De/serialization not supported for MockSource".to_string())),

            #[cfg(any(test, feature = "fakes"))]
            sources::Source::FakeSource(_) => Err(Error::SerializationError("De/serialization not supported for FakeSource".to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::SourceTrait;

    #[test]
    fn test_basicsample() {
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
            samples::Sample::BaseSample(s) => {
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

    #[test]
    fn test_fs_source() {
        let name = Some(String::from("Name"));
        let uuid = Uuid::new_v4();
        let path = String::from("/home");
        let uri = String::from("file:///home");
        let exts = vec![String::from("wav"), String::from("ogg")];
        let enabled = true;

        let x = Source::FilesystemSourceV1(FilesystemSourceV1 {
            name: name.clone(),
            uuid,
            path: path.clone(),
            uri: uri.clone(),
            exts: exts.clone(),
            enabled,
        });

        let encoded = serde_json::to_string(&x).unwrap();
        let decoded = serde_json::from_str::<Source>(&encoded).unwrap();

        match &decoded {
            Source::FilesystemSourceV1(decoded_src) => {
                assert_eq!(decoded_src.name, name);
                assert_eq!(decoded_src.uuid, uuid);
                assert_eq!(decoded_src.path, path);
                assert_eq!(decoded_src.uri, uri);
                assert_eq!(decoded_src.exts, exts);
                assert_eq!(decoded_src.enabled, enabled);
            }

            #[allow(unreachable_patterns)]
            _ => panic!(),
        }

        let domained = decoded.clone().try_into_domain().unwrap();

        match domained {
            sources::Source::FilesystemSource(domained_src) => {
                assert_eq!(domained_src.name(), name.as_deref());
                assert_eq!(domained_src.uuid(), &uuid);
                assert_eq!(domained_src.uri(), uri);
                assert_eq!(domained_src.is_enabled(), enabled);
            }

            _ => panic!(),
        }
    }
}
