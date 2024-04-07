// MIT License
// 
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use std::path::Path;

use uuid::Uuid;

use crate::prelude::*;
use crate::errors::{Error, LogDiscard};
use crate::samples::{BasicSample, Sample};

pub mod io;

use self::io::{DefaultIO, IO};

use super::SourceReader;

#[derive(Debug)]
pub struct FilesystemSource<T>
where
    T: IO,
{
    io: T,
    name: Option<String>,
    uuid: Uuid,
    path: String,
    uri: String,
    _exts: Vec<String>,
}

impl FilesystemSource<DefaultIO> {
    pub fn new(path: String, exts: Vec<String>) -> Self {
        FilesystemSource::new_with_io(None, path, exts, DefaultIO())
    }

    pub fn new_named(name: String, path: String, exts: Vec<String>) -> Self {
        FilesystemSource::new_with_io(Some(name), path, exts, DefaultIO())
    }
}

impl<T> FilesystemSource<T>
where
    T: IO,
{
    pub fn new_with_io(name: Option<String>, path: String, exts: Vec<String>, io: T) -> FilesystemSource<T> {
        let uri = format!("file://{path}");
        FilesystemSource {
            io,
            name,
            uuid: Uuid::new_v4(),
            path,
            uri,
            _exts: exts,
        }
    }

    pub fn sample_from_path(&self, path: &Path) -> Result<Sample, Error> {
        match (self.io.is_file(path), path.to_str()) {
            (true, Some(s)) => Ok(Sample::BasicSample(BasicSample::new(
                s.to_string(),
                path.file_name()
                    .and_then(|name| name.to_str())
                    .expect("file has valid UTF-8 name due to is_file and path.to_str")
                    .to_string(),
                self.io.metadata(path)?,
                Some(self.uuid),
            ))),
            (false, Some(s)) => Err(Error::IoError(s, "Not a regular file")),
            (_, None) => Err(Error::IoError("{n/a}", "Invalid UTF-8 in path")),
        }
    }
}

impl<T> SourceTrait for FilesystemSource<T>
where
    T: IO,
{
    fn name(&self) -> Option<&str> {
        self.name.as_ref().map(|s| s.as_str())
    }

    fn uri(&self) -> &str {
        &self.uri
    }

    fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    fn list(&self) -> Result<Vec<Sample>, Error> {
        // TODO: use .exts
        Ok(self
            .io
            .glob(format!("{}/**/*.wav", self.path).as_str())?
            .log_and_discard_errors(log::Level::Error)
            .map(|path| self.sample_from_path(&path))
            .log_and_discard_errors(log::Level::Error)
            .collect())
    }

    fn stream(&self, sample: &Sample) -> Result<SourceReader, Error> {
        // TODO: verify starts with "file://", then drop prefix before using with Path::new
        self.io.stream(Path::new(sample.uri()))
    }
}

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, path::PathBuf, str::FromStr};

    use crate::samples::SampleMetadata;

    use super::*;
    use mockall::predicate;

    trait UnwrapInfallible<T> {
        fn unwrap_infallible(self) -> T;
    }

    impl<T> UnwrapInfallible<T> for Result<T, Infallible> {
        fn unwrap_infallible(self) -> T {
            self.expect("infallible operation")
        }
    }

    #[test]
    fn test_list() {
        macro_rules! path {
            ($e: expr) => {
                PathBuf::from_str($e).unwrap_infallible()
            };
        }

        let mut mockio = io::MockIO::default();

        mockio
            .expect_glob()
            .with(predicate::eq("/samples/**/*.wav"))
            .returning(|_| {
                Ok(vec![
                    Ok(path!("/samples/first.wav")),
                    Ok(path!("/samples/second.wav")),
                    Err(Error::IoError {
                        uri: String::from("bad uri"),
                        details: String::from("random error"),
                    }),
                    Ok(path!("/samples/third.wav")),
                    Ok(path!("/samples/__MACOSX/.third.wav")),
                ]
                .into_iter())
            });

        mockio
            .expect_is_file()
            .returning(|path| path.to_str() != Some("/samples/__MACOSX/.third.wav"));

        mockio.expect_metadata().returning(|_| {
            Ok(SampleMetadata {
                rate: 44100,
                channels: 2,
                src_fmt_display: String::from("PCM S16LE"),
            })
        });

        let src = FilesystemSource::new_with_io(None, String::from("/samples"), vec![], mockio);

        assert_eq!(src.list().expect("three non-error results").len(), 3);
    }
}
