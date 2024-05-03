// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use std::path::Path;

use uuid::Uuid;

use crate::errors::{Error, LogDiscard};
use crate::prelude::*;
use crate::samples::{BaseSample, Sample, SampleURI};

pub mod io;

use self::io::{DefaultIO, IO};

use super::SourceReader;

#[derive(Debug, Clone)]
pub struct FilesystemSource<T>
where
    T: IO,
{
    pub(crate) io: T,
    pub(crate) name: Option<String>,
    pub(crate) uuid: Uuid,
    pub(crate) path: String,
    pub(crate) uri: String,
    pub(crate) exts: Vec<String>,
    pub(crate) enabled: bool,
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
    pub fn new_with_io(
        name: Option<String>,
        path: String,
        exts: Vec<String>,
        io: T,
    ) -> FilesystemSource<T> {
        let uri = format!("file://{path}");
        FilesystemSource {
            io,
            name,
            uuid: Uuid::new_v4(),
            path,
            uri,
            exts,
            enabled: true,
        }
    }

    pub fn sample_from_path(&self, path: &Path) -> Result<Sample, Error> {
        match (self.io.is_file(path), path.to_str()) {
            (true, Some(s)) => Ok(Sample::BaseSample(BaseSample::new(
                &SampleURI(format!("file://{s}")),
                &path.file_name()
                    .and_then(|name| name.to_str())
                    .expect("file has valid UTF-8 name due to is_file and path.to_str")
                    .to_string(),
                &self.io.metadata(path)?,
                Some(self.uuid),
            ))),
            (false, Some(s)) => Err(Error::IoError(s, "Not a regular file")),
            (_, None) => Err(Error::IoError("{n/a}", "Invalid UTF-8 in path")),
        }
    }

    pub(crate) fn set_uuid(&mut self, uuid: Uuid) {
        self.uuid = uuid;
    }
}

impl<T> PartialEq for FilesystemSource<T>
where
    T: IO,
{
    fn eq(&self, other: &Self) -> bool {
        (&self.name, &self.path, &self.uuid) == (&other.name, &other.path, &other.uuid)
    }
}

impl<T> SourceOps for FilesystemSource<T>
where
    T: IO + std::fmt::Debug,
{
    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    fn uri(&self) -> &str {
        &self.uri
    }

    fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    fn list(&self) -> Result<Vec<Sample>, Error> {
        let mut result = Vec::<_>::new();

        for ext in self.exts.iter() {
            result.extend(
                self.io
                    .glob(format!("{}/**/*.{ext}", self.path).as_str())?
                    .log_and_discard_errors(log::Level::Error)
                    .map(|path| self.sample_from_path(&path))
                    .log_and_discard_errors(log::Level::Error),
            )
        }

        Ok(result)
    }

    fn stream(&self, sample: &Sample) -> Result<SourceReader, Error> {
        if sample.uri().starts_with("file://") {
            self.io
                .stream(Path::new(&String::from_iter(sample.uri().chars().skip(7))))
        } else {
            Err(Error::SourceInvalidUriError {
                uri: sample.uri().to_string(),
                source_type: String::from("FilesystemSource"),
            })
        }
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn enable(&mut self) {
        self.enabled = true;
    }

    fn disable(&mut self) {
        self.enabled = false;
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

        fn mock() -> io::MockIO {
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
                .expect_glob()
                .with(predicate::eq("/samples/**/*.ogg"))
                .returning(|_| Ok(vec![Ok(path!("/samples/first.ogg"))].into_iter()));

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
            mockio
        }

        let src = FilesystemSource::new_with_io(
            None,
            String::from("/samples"),
            vec!["wav".to_string()],
            mock(),
        );
        assert_eq!(src.list().expect("three non-error results").len(), 3);

        let src = FilesystemSource::new_with_io(
            None,
            String::from("/samples"),
            vec!["ogg".to_string()],
            mock(),
        );
        assert_eq!(src.list().expect("one non-error results").len(), 1);

        let src = FilesystemSource::new_with_io(
            None,
            String::from("/samples"),
            vec!["wav".to_string(), "ogg".to_string()],
            mock(),
        );
        assert_eq!(src.list().expect("four non-error results").len(), 4);
    }
}
