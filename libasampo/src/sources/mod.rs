// MIT License
// 
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use std::fs::File;
use std::io::Read;

use uuid::Uuid;

use crate::errors::Error;
use crate::samples::Sample;

pub mod file_system_source;

pub trait SourceReaderTrait: Read {}

pub enum SourceReader {
    FileReader(File),
    VecReader(Vec<f32>),
    NullReader(),
}

impl From<File> for SourceReader {
    fn from(value: File) -> Self {
        SourceReader::FileReader(value)
    }
}

impl Read for SourceReader {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            SourceReader::FileReader(_) => unimplemented!(),
            SourceReader::VecReader(_) => unimplemented!(),
            Self::NullReader() => unimplemented!(),
        }
    }
}

impl SourceReaderTrait for SourceReader {}

pub trait SourceTrait {
    fn name(&self) -> Option<&str>;
    fn uri(&self) -> &str;
    fn uuid(&self) -> &Uuid;
    fn list(&self) -> Result<Vec<Sample>, Error>;
    fn stream(&self, sample: &Sample) -> Result<SourceReader, Error>;
    fn is_enabled(&self) -> bool;
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
