use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::sources::{
    file_system_source::{
        io::{DefaultIO, IO},
        FilesystemSource as RealFilesystemSource,
    },
    Source as RealSource,
};

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

impl IntoDomain<RealSource> for FilesystemSourceV1 {
    fn into_domain(self) -> RealSource {
        let mut src =
            RealFilesystemSource::new_with_io(self.name, self.path, self.exts, DefaultIO());
        src.set_uuid(self.uuid);
        RealSource::FilesystemSource(src)
    }
}

impl<T: IO> From<RealFilesystemSource<T>> for FilesystemSourceV1 {
    fn from(src: RealFilesystemSource<T>) -> Self {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Source {
    FilesystemSourceV1(FilesystemSourceV1),
}

impl IntoDomain<RealSource> for Source {
    fn into_domain(self) -> RealSource {
        match self {
            Source::FilesystemSourceV1(src) => src.into_domain(),
        }
    }
}

impl From<RealSource> for Source {
    fn from(value: RealSource) -> Self {
        match value {
            RealSource::FilesystemSource(src) => {
                Source::FilesystemSourceV1(FilesystemSourceV1::from(src))
            }

            #[cfg(feature = "mocks")]
            RealSource::MockSource(_) => unimplemented!(),

            #[cfg(feature = "fakes")]
            RealSource::FakeSource(_) => unimplemented!(),
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
        }

        match domained {
            RealSource::FilesystemSource(domained_src) => {
                assert_eq!(domained_src.name(), name.as_deref());
                assert_eq!(domained_src.uuid(), &uuid);
                assert_eq!(domained_src.uri(), uri);
                assert_eq!(domained_src.is_enabled(), enabled);
            }

            #[cfg(feature = "mocks")]
            RealSource::MockSource(_) => unimplemented!(),

            #[cfg(feature = "fakes")]
            RealSource::FakeSource(_) => unimplemented!(),
        }
    }
}
