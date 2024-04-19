// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(feature = "fakes")]
use std::collections::HashMap;

use crate::sources;
use crate::sources::file_system_source as fs_source;

#[cfg(feature = "fakes")]
use crate::samples::Sample;

pub trait IntoDomain<T> {
    fn into_domain(self) -> T;
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

impl IntoDomain<sources::Source> for FilesystemSourceV1 {
    fn into_domain(self) -> sources::Source {
        let mut src = fs_source::FilesystemSource::new_with_io(
            self.name,
            self.path,
            self.exts,
            fs_source::io::DefaultIO(),
        );
        src.set_uuid(self.uuid);
        sources::Source::FilesystemSource(src)
    }
}

impl<T: fs_source::io::IO> From<fs_source::FilesystemSource<T>> for FilesystemSourceV1 {
    fn from(src: fs_source::FilesystemSource<T>) -> Self {
        FilesystemSourceV1 {
            name: src.name.clone(),
            uuid: src.uuid,
            path: src.path.clone(),
            uri: src.uri.clone(),
            exts: src.exts.clone(),
            enabled: src.enabled,
        }
    }
}

#[cfg(feature = "fakes")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FakeSourceV1 {
    pub name: Option<String>,
    pub uri: String,
    pub uuid: Uuid,
    pub list: Vec<Sample>,
    pub stream: HashMap<Sample, Vec<f32>>,
    pub enabled: bool,
}

#[cfg(feature = "fakes")]
impl IntoDomain<sources::Source> for FakeSourceV1 {
    fn into_domain(self) -> sources::Source {
        sources::Source::FakeSource(sources::FakeSource {
            name: self.name,
            uri: self.uri,
            uuid: self.uuid,
            list: self.list,
            list_error: None,
            stream: self.stream,
            stream_error: None,
            enabled: self.enabled,
        })
    }
}

#[cfg(feature = "fakes")]
impl From<sources::FakeSource> for FakeSourceV1 {
    fn from(value: sources::FakeSource) -> Self {
        if value.list_error.is_some() || value.stream_error.is_some() {
            panic!("Cannot serialize fake source with errors");
        }

        FakeSourceV1 {
            name: value.name,
            uri: value.uri,
            uuid: value.uuid,
            list: value.list,
            stream: value.stream,
            enabled: value.enabled,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Source {
    FilesystemSourceV1(FilesystemSourceV1),

    #[cfg(feature = "fakes")]
    FakeSourceV1(FakeSourceV1),
}

impl IntoDomain<sources::Source> for Source {
    fn into_domain(self) -> sources::Source {
        match self {
            Source::FilesystemSourceV1(src) => src.into_domain(),

            #[cfg(feature = "fakes")]
            Source::FakeSourceV1(src) => src.into_domain(),
        }
    }
}

impl From<sources::Source> for Source {
    fn from(value: sources::Source) -> Self {
        match value {
            sources::Source::FilesystemSource(src) => {
                Source::FilesystemSourceV1(FilesystemSourceV1::from(src))
            }

            #[cfg(feature = "mocks")]
            sources::Source::MockSource(_) => unimplemented!(),

            #[cfg(feature = "fakes")]
            sources::Source::FakeSource(src) => Source::FakeSourceV1(FakeSourceV1::from(src)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::SourceTrait;

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
            uuid: uuid.clone(),
            path: path.clone(),
            uri: uri.clone(),
            exts: exts.clone(),
            enabled,
        });

        let encoded = serde_json::to_string(&x).unwrap();
        let decoded = serde_json::from_str::<Source>(&encoded).unwrap();
        let domained = decoded.clone().into_domain();

        match decoded {
            Source::FilesystemSourceV1(decoded_src) => {
                assert_eq!(decoded_src.name, name);
                assert_eq!(decoded_src.uuid, uuid);
                assert_eq!(decoded_src.path, path);
                assert_eq!(decoded_src.uri, uri);
                assert_eq!(decoded_src.exts, exts);
                assert_eq!(decoded_src.enabled, enabled);
            }

            #[cfg(feature = "fakes")]
            Source::FakeSourceV1(_decoded_src) => unimplemented!(),
        }

        match domained {
            sources::Source::FilesystemSource(domained_src) => {
                assert_eq!(domained_src.name(), name.as_deref());
                assert_eq!(domained_src.uuid(), &uuid);
                assert_eq!(domained_src.uri(), uri);
                assert_eq!(domained_src.is_enabled(), enabled);
            }

            #[cfg(feature = "mocks")]
            sources::Source::MockSource(_) => unimplemented!(),

            #[cfg(feature = "fakes")]
            sources::Source::FakeSource(_) => unimplemented!(),
        }
    }
}
