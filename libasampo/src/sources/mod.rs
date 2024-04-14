// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use std::fs::File;
use std::io::{Read, Seek};

use uuid::Uuid;

use crate::errors::Error;
use crate::samples::Sample;

pub mod file_system_source;

pub trait SourceReaderTrait: Read + Seek {}

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
            },
            Self::NullReader() => unimplemented!(),
        }
    }
}

impl Seek for SourceReader {
    fn seek(&mut self, spec: std::io::SeekFrom) -> std::io::Result<u64> {
        match self {
            SourceReader::FileReader(fd) => fd.seek(spec),

            SourceReader::VecReader(v, pos) => {
                match spec {
                    std::io::SeekFrom::Start(to) => {
                        *pos = core::cmp::min(to as usize, v.len());
                        Ok(4 * (*pos as u64))
                    },

                    std::io::SeekFrom::End(to) => {
                        *pos = core::cmp::max(0, *pos - (to as usize));
                        Ok(4 * (*pos as u64))
                    },

                    std::io::SeekFrom::Current(to) => {
                        *pos = core::cmp::min(*pos + (to as usize), v.len());
                        Ok(4 * (*pos as u64))
                    },
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

// TODO: use enum-dispatch
pub enum Source {
    FilesystemSource(file_system_source::FilesystemSource<file_system_source::io::DefaultIO>),
}

impl SourceTrait for Source {
    fn name(&self) -> Option<&str> {
        match self {
            Self::FilesystemSource(src) => src.name(),
        }
    }
    fn uri(&self) -> &str {
        match self {
            Self::FilesystemSource(src) => src.uri(),
        }
    }

    fn uuid(&self) -> &Uuid {
        match self {
            Self::FilesystemSource(src) => src.uuid(),
        }
    }

    fn list(&self) -> Result<Vec<Sample>, Error> {
        match self {
            Self::FilesystemSource(src) => src.list(),
        }
    }

    fn stream(&self, sample: &Sample) -> Result<SourceReader, Error> {
        match self {
            Self::FilesystemSource(src) => src.stream(sample),
        }
    }

    fn is_enabled(&self) -> bool {
        match self {
            Self::FilesystemSource(src) => src.is_enabled(),
        }
    }

    fn set_enabled(&mut self, enabled: bool) {
        match self {
            Self::FilesystemSource(src) => src.set_enabled(enabled),
        }
    }

    fn enable(&mut self) {
        match self {
            Self::FilesystemSource(src) => src.enable(),
        }
    }

    fn disable(&mut self) {
        match self {
            Self::FilesystemSource(src) => src.disable(),
        }
    }
}

impl Clone for Source {
    fn clone(&self) -> Self {
        match self {
            Self::FilesystemSource(src) => Self::FilesystemSource(src.clone()),
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
        }
    }
}
