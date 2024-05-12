// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use std::fs::File;
use std::io::{Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[cfg(test)]
use std::vec::IntoIter;

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

pub trait IO: Clone + std::fmt::Debug {
    type Paths: Iterator<Item = Result<PathBuf, Error>>;

    // These could be static, but mockall is nicer to use with non-static methods.
    fn glob(&self, pattern: &str) -> Result<Self::Paths, Error>;
    fn is_file(&self, path: &Path) -> bool;
    fn stream(&self, path: &Path) -> Result<SourceReader, Error>;
    fn raw_copy<T: 'static + std::io::Write>(&self, src: &Path, dst: &mut T) -> Result<(), Error>;
    fn metadata(&self, path: &Path) -> Result<SampleMetadata, Error>;
}

#[derive(Debug, Clone)]
pub struct DefaultIO();

#[cfg(test)]
mockall::mock! {
    pub IO { }

    impl IO for IO {
        type Paths = IntoIter<Result<PathBuf, Error>>;

        fn glob(&self, pattern: &str) -> Result<<Self as IO>::Paths, Error>;
        fn is_file(&self, path: &Path) -> bool;
        fn stream(&self, path: &Path) -> Result<SourceReader, Error>;
        fn raw_copy<T: 'static + std::io::Write>(&self, src: &Path, dst: &mut T) -> Result<(), Error>;
        fn metadata(&self, path: &Path) -> Result<SampleMetadata, Error>;
    }

    impl Clone for IO {
        fn clone(&self) -> Self;
    }

    impl std::fmt::Debug for IO {
        fn fmt<'a>(&self, f: &mut std::fmt::Formatter<'a>) -> std::fmt::Result;
    }
}

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

    fn raw_copy<T: 'static + std::io::Write>(&self, src: &Path, dst: &mut T) -> Result<(), Error> {
        Ok(std::io::copy(
            &mut File::open(src).map_err(|e| Error::IoError {
                uri: src.to_string_lossy().to_string(),
                details: e.to_string(),
            })?,
            dst,
        )
        .map(|_| ())
        .map_err(|e| Error::IoError {
            uri: src.to_string_lossy().to_string(),
            details: e.to_string(),
        })?)
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
                    .ok_or(Error::io_error(
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

                    size_bytes: {
                        File::open(path).map_or(None, |mut fd| fd.seek(SeekFrom::End(0)).ok())
                    },

                    length_millis: match (codec_params.time_base, codec_params.n_frames) {
                        (Some(timebase), Some(n)) => {
                            let time = timebase.calc_time(n);
                            Some(time.seconds * 1000 + ((time.frac * 1000.0) as u64))
                        }
                        _ => None,
                    },
                })
            }
            Err(e) => Err(e.into()),
        }
    }
}
