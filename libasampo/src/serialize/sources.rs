// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{errors::Error, serialize::TryIntoDomain, sources::file_system_source as fs_source};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemSourceV1 {
    name: Option<String>,
    uuid: Uuid,
    path: String,
    uri: String,
    exts: Vec<String>,
    enabled: bool,
}

impl TryIntoDomain<crate::sources::Source> for FilesystemSourceV1 {
    fn try_into_domain(self) -> Result<crate::sources::Source, Error> {
        let mut src = fs_source::FilesystemSource::new_with_io(
            self.name,
            self.path,
            self.exts,
            fs_source::io::DefaultIO(),
        );
        src.set_uuid(self.uuid);
        Ok(crate::sources::Source::FilesystemSource(src))
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

impl TryIntoDomain<crate::sources::Source> for Source {
    fn try_into_domain(self) -> Result<crate::sources::Source, Error> {
        match self {
            Source::FilesystemSourceV1(src) => src.try_into_domain(),
        }
    }
}

impl TryFrom<crate::sources::Source> for Source {
    type Error = crate::errors::Error;

    fn try_from(value: crate::sources::Source) -> Result<Self, Error> {
        match value {
            crate::sources::Source::FilesystemSource(src) => Ok(Source::FilesystemSourceV1(
                FilesystemSourceV1::try_from(src)?,
            )),

            #[cfg(feature = "mocks")]
            crate::sources::Source::MockSource(_) => Err(Error::SerializationError(
                "De/serialization not supported for MockSource".to_string(),
            )),

            #[cfg(any(test, feature = "fakes"))]
            crate::sources::Source::FakeSource(_) => Err(Error::SerializationError(
                "De/serialization not supported for FakeSource".to_string(),
            )),
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
            crate::sources::Source::FilesystemSource(domained_src) => {
                assert_eq!(domained_src.name(), name.as_deref());
                assert_eq!(domained_src.uuid(), &uuid);
                assert_eq!(domained_src.uri(), uri);
                assert_eq!(domained_src.is_enabled(), enabled);
            }

            _ => panic!(),
        }
    }
}
