// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

#[cfg(any(test, feature = "fakes"))]
use std::collections::HashMap;

use std::fs::File;
use std::io::{Read, Seek};

use uuid::Uuid;

use crate::errors::Error;
use crate::samples::Sample;

#[cfg(any(test, feature = "fakes"))]
use crate::samples::SampleTrait;

pub mod file_system_source;

pub trait SourceReaderTrait: Read + Seek {}

#[derive(Debug)]
pub enum SourceReader {
    FileReader(File),
    VecReader(Vec<f32>, usize),
    NullReader(),
}

impl From<File> for SourceReader {
    fn from(value: File) -> Self {
        SourceReader::FileReader(value)
    }
}

impl Read for SourceReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            SourceReader::FileReader(fd) => fd.read(buf),
            SourceReader::VecReader(v, pos) => {
                if *pos < v.len() {
                    unsafe {
                        let (_, buf_f32, _) = buf.align_to_mut::<f32>();
                        let floats_to_write = core::cmp::min(buf_f32.len(), v.len() - *pos);
                        buf_f32.copy_from_slice(&v[*pos..(*pos + floats_to_write)]);
                        *pos += buf_f32.len();
                        Ok(4 * floats_to_write)
                    }
                } else {
                    Ok(0)
                }
            }
            Self::NullReader() => unimplemented!(),
        }
    }
}

impl Seek for SourceReader {
    fn seek(&mut self, spec: std::io::SeekFrom) -> std::io::Result<u64> {
        match self {
            SourceReader::FileReader(fd) => fd.seek(spec),

            SourceReader::VecReader(v, pos) => match spec {
                std::io::SeekFrom::Start(to) => {
                    *pos = core::cmp::min(to as usize, v.len());
                    Ok(4 * (*pos as u64))
                }

                std::io::SeekFrom::End(to) => {
                    *pos = core::cmp::max(0, *pos - (to as usize));
                    Ok(4 * (*pos as u64))
                }

                std::io::SeekFrom::Current(to) => {
                    *pos = core::cmp::min(*pos + (to as usize), v.len());
                    Ok(4 * (*pos as u64))
                }
            },

            SourceReader::NullReader() => unimplemented!(),
        }
    }
}

impl SourceReaderTrait for SourceReader {}

pub trait SourceTrait: PartialEq + Clone + std::fmt::Debug {
    fn name(&self) -> Option<&str>;
    fn uri(&self) -> &str;
    fn uuid(&self) -> &Uuid;
    fn list(&self) -> Result<Vec<Sample>, Error>;
    fn stream(&self, sample: &Sample) -> Result<SourceReader, Error>;
    fn is_enabled(&self) -> bool;
    fn set_enabled(&mut self, enabled: bool);
    fn enable(&mut self);
    fn disable(&mut self);
}

#[cfg(feature = "mocks")]
mockall::mock! {
    pub Source { }

    impl SourceTrait for Source {
        fn name<'a>(&'a self) -> Option<&'a str>;
        fn uri(&self) -> &str;
        fn uuid(&self) -> &Uuid;
        fn list(&self) -> Result<Vec<Sample>, Error>;
        fn stream(&self, sample: &Sample) -> Result<SourceReader, Error>;
        fn is_enabled(&self) -> bool;
        fn set_enabled(&mut self, enabled: bool);
        fn enable(&mut self);
        fn disable(&mut self);
    }

    impl PartialEq for Source {
        fn eq(&self, other: &MockSource) -> bool;
    }

    impl Clone for Source {
        fn clone(&self) -> Self;
    }

    impl std::fmt::Debug for Source {
        fn fmt<'a>(&self, f: &mut std::fmt::Formatter<'a>) -> std::fmt::Result;
    }
}

#[cfg(any(test, feature = "fakes"))]
#[derive(PartialEq)]
pub struct FakeSource {
    pub name: Option<String>,
    pub uri: String,
    pub uuid: Uuid,
    pub list: Vec<Sample>,
    pub list_error: Option<fn() -> Error>,
    pub stream: HashMap<Sample, Vec<f32>>,
    pub stream_error: Option<fn() -> Error>,
    pub enabled: bool,
}

// TODO: use enum-dispatch
pub enum Source {
    FilesystemSource(file_system_source::FilesystemSource<file_system_source::io::DefaultIO>),

    #[cfg(feature = "mocks")]
    MockSource(MockSource),

    #[cfg(any(test, feature = "fakes"))]
    FakeSource(FakeSource),
}

impl SourceTrait for Source {
    fn name(&self) -> Option<&str> {
        match self {
            Self::FilesystemSource(src) => src.name(),

            #[cfg(feature = "mocks")]
            Self::MockSource(src) => src.name(),

            #[cfg(any(test, feature = "fakes"))]
            Self::FakeSource(src) => src.name.as_deref(),
        }
    }
    fn uri(&self) -> &str {
        match self {
            Self::FilesystemSource(src) => src.uri(),

            #[cfg(feature = "mocks")]
            Self::MockSource(src) => src.uri(),

            #[cfg(any(test, feature = "fakes"))]
            Self::FakeSource(src) => &src.uri,
        }
    }

    fn uuid(&self) -> &Uuid {
        match self {
            Self::FilesystemSource(src) => src.uuid(),

            #[cfg(feature = "mocks")]
            Self::MockSource(src) => src.uuid(),

            #[cfg(any(test, feature = "fakes"))]
            Self::FakeSource(src) => &src.uuid,
        }
    }

    fn list(&self) -> Result<Vec<Sample>, Error> {
        match self {
            Self::FilesystemSource(src) => src.list(),

            #[cfg(feature = "mocks")]
            Self::MockSource(src) => src.list(),

            #[cfg(any(test, feature = "fakes"))]
            Self::FakeSource(src) => match &src.list_error {
                Some(error) => Err(error()),
                None => Ok(src.list.clone()),
            },
        }
    }

    fn stream(&self, sample: &Sample) -> Result<SourceReader, Error> {
        match self {
            Self::FilesystemSource(src) => src.stream(sample),

            #[cfg(feature = "mocks")]
            Self::MockSource(src) => src.stream(sample),

            #[cfg(any(test, feature = "fakes"))]
            Self::FakeSource(src) => match &src.stream_error {
                Some(error) => Err(error()),
                None => match src.stream.get(sample) {
                    Some(vec) => Ok(SourceReader::VecReader(vec.clone(), 0)),
                    None => Err(Error::IoError {
                        uri: sample.uri().to_string(),
                        details: String::from("???"),
                    }),
                },
            },
        }
    }

    fn is_enabled(&self) -> bool {
        match self {
            Self::FilesystemSource(src) => src.is_enabled(),

            #[cfg(feature = "mocks")]
            Self::MockSource(src) => src.is_enabled(),

            #[cfg(any(test, feature = "fakes"))]
            Self::FakeSource(src) => src.enabled,
        }
    }

    fn set_enabled(&mut self, enabled: bool) {
        match self {
            Self::FilesystemSource(src) => src.set_enabled(enabled),

            #[cfg(feature = "mocks")]
            Self::MockSource(src) => src.set_enabled(enabled),

            #[cfg(any(test, feature = "fakes"))]
            Self::FakeSource(src) => src.enabled = enabled,
        }
    }

    fn enable(&mut self) {
        match self {
            Self::FilesystemSource(src) => src.enable(),

            #[cfg(feature = "mocks")]
            Self::MockSource(src) => src.enable(),

            #[cfg(any(test, feature = "fakes"))]
            Self::FakeSource(src) => src.enabled = true,
        }
    }

    fn disable(&mut self) {
        match self {
            Self::FilesystemSource(src) => src.disable(),

            #[cfg(feature = "mocks")]
            Self::MockSource(src) => src.disable(),

            #[cfg(any(test, feature = "fakes"))]
            Self::FakeSource(src) => src.enabled = false,
        }
    }
}

impl Clone for Source {
    fn clone(&self) -> Self {
        match self {
            Self::FilesystemSource(src) => Self::FilesystemSource(src.clone()),

            #[cfg(feature = "mocks")]
            Self::MockSource(src) => Self::MockSource(src.clone()),

            #[cfg(any(test, feature = "fakes"))]
            Self::FakeSource(src) => Self::FakeSource(FakeSource {
                name: src.name.clone(),
                uri: src.uri.clone(),
                uuid: src.uuid,
                list: src.list.clone(),
                list_error: src.list_error,
                stream: src.stream.clone(),
                stream_error: src.stream_error,
                enabled: src.enabled,
            }),
        }
    }
}

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Source")
    }
}

impl std::cmp::PartialEq for Source {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::FilesystemSource(left), Self::FilesystemSource(right)) => left == right,

            #[cfg(feature = "mocks")]
            (Self::MockSource(left), Self::MockSource(right)) => left == right,

            #[cfg(any(test, feature = "fakes"))]
            (Self::FakeSource(left), Self::FakeSource(right)) => left == right,

            #[cfg(any(test, feature = "mocks", feature = "fakes"))]
            _ => false,
        }
    }
}
