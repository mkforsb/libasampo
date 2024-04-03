// MIT License
// 
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use std::fs::File;
use std::path::{Path, PathBuf};
use std::vec::IntoIter;

use mockall::automock;

use crate::errors::Error;
use crate::samples::SampleMetadata;
use crate::sources::SourceReader;

pub struct GlobPathsWithMappedError(glob::Paths);

impl Iterator for GlobPathsWithMappedError {
    type Item = Result<PathBuf, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|x| x.map_err(Error::from))
    }
}

#[automock(type Paths=IntoIter<Result<PathBuf, Error>>;)]
pub trait IO {
    type Paths: Iterator<Item = Result<PathBuf, Error>>;

    // These could be static, but mockall is nicer to use with non-static methods.
    fn glob(&self, pattern: &str) -> Result<Self::Paths, Error>;
    fn is_file(&self, path: &Path) -> bool;
    fn stream(&self, path: &Path) -> Result<SourceReader, Error>;
    fn metadata(&self, path: &Path) -> Result<SampleMetadata, Error>;
}

#[derive(Debug)]
pub struct DefaultIO();

impl IO for DefaultIO {
    type Paths = GlobPathsWithMappedError;

    fn glob(&self, pattern: &str) -> Result<Self::Paths, Error> {
        match glob::glob(pattern) {
            Ok(paths) => Ok(GlobPathsWithMappedError(paths)),
            Err(e) => Err(Error::from(e)),
        }
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn stream(&self, path: &Path) -> Result<SourceReader, Error> {
        Ok(File::open(path)?.into())
    }

    fn metadata(&self, path: &Path) -> Result<SampleMetadata, Error> {
        use symphonia::core::{io::MediaSourceStream, probe::Hint};

        let mss = MediaSourceStream::new(Box::new(File::open(path)?), Default::default());
        let mut hint = Hint::new();

        if let Some(ext) = path.extension() {
            hint.with_extension(&ext.to_string_lossy());
        }

        match symphonia::default::get_probe().format(
            &hint,
            mss,
            &Default::default(),
            &Default::default(),
        ) {
            Ok(probed) => {
                let codec_params = &probed
                    .format
                    .default_track()
                    .ok_or(Error::IoError(
                        path.to_string_lossy(),
                        "Symphonia format error: No default track",
                    ))?
                    .codec_params;

                Ok(SampleMetadata {
                    // TODO: better way of indicating "unknown" sample rate.
                    rate: codec_params.sample_rate.unwrap_or(0),

                    // TODO: better way o indicating "unknown" channel count.
                    channels: codec_params.channels.map_or(0, |ch| ch.count() as u8),

                    // TODO: implement properly
                    src_fmt_display: "readable format info".to_string(),
                })
            }
            Err(e) => Err(e.into()),
        }
    }
}
