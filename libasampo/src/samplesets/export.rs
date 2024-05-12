// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use std::{collections::HashMap, fs::File, io::Write, path::Path};

use uuid::Uuid;

use crate::{
    errors::Error,
    prelude::{SampleOps, SourceOps},
    samplesets::{SampleSet, SampleSetOps},
    sources::Source,
};

pub trait IO {
    type Writable: 'static + Write;

    fn create_dir_all(&mut self, path: &Path) -> Result<(), Error>;
    fn file_create(&mut self, path: &Path) -> Result<Self::Writable, Error>;
}

struct DefaultIO;

impl IO for DefaultIO {
    type Writable = File;

    fn create_dir_all(&mut self, path: &Path) -> Result<(), Error> {
        Ok(std::fs::create_dir_all(path)?)
    }

    fn file_create(&mut self, path: &Path) -> Result<Self::Writable, Error> {
        Ok(File::create(path)?)
    }
}

#[derive(Debug, Clone)]
pub enum Conversion {
    Wav { rate: u32, depth: u8, channels: u8 },
}

#[derive(Debug, Clone)]
pub struct ExportJob<T>
where
    T: IO,
{
    pub io: T,
    pub target_directory: String,
    pub conversion: Option<Conversion>,
}

impl ExportJob<DefaultIO> {
    pub fn new(target_directory: impl Into<String>, conversion: Option<Conversion>) -> Self {
        ExportJob {
            io: DefaultIO,
            target_directory: target_directory.into(),
            conversion,
        }
    }
}

impl<T> ExportJob<T>
where
    T: IO,
{
    pub fn perform(&mut self, sampleset: &SampleSet, sources: &[&Source]) -> Result<(), Error> {
        let sourcemap = sources
            .iter()
            .map(|src| (*src.uuid(), *src))
            .collect::<HashMap<Uuid, &Source>>();

        let target_path = Path::new(&self.target_directory);

        self.io
            .create_dir_all(target_path)
            .map_err(|e| Error::IoError {
                uri: target_path.to_string_lossy().to_string(),
                details: e.to_string(),
            })?;

        for sample in sampleset.list().iter() {
            let uuid = sample
                .source_uuid()
                .ok_or(Error::SampleMissingSourceUUIDError(
                    sample.uri().to_string(),
                ))?;

            let mut filename = target_path.to_path_buf();
            filename.push(sample.name());

            let mut dst = self.io.file_create(&filename).map_err(|e| Error::IoError {
                uri: filename.to_string_lossy().to_string(),
                details: e.to_string(),
            })?;

            sourcemap
                .get(uuid)
                .ok_or(Error::MissingSourceError(*uuid))?
                .raw_copy(sample, &mut dst)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use crate::{
        samplesets::BaseSampleSet,
        testutils::{self, fakesource_from_json, s},
    };

    use super::*;

    #[derive(Debug, Clone)]
    struct MockIOWritable(Rc<RefCell<Vec<u8>>>);

    #[derive(Debug, Clone)]
    struct MockIO {
        pub writable: HashMap<String, MockIOWritable>,
    }

    impl std::io::Write for MockIOWritable {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.borrow_mut().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl IO for MockIO {
        type Writable = MockIOWritable;

        fn create_dir_all(&mut self, _path: &Path) -> Result<(), Error> {
            Ok(())
        }

        fn file_create(&mut self, path: &Path) -> Result<Self::Writable, Error> {
            let writable = MockIOWritable(Rc::new(RefCell::new(Vec::new())));

            self.writable
                .insert(path.to_string_lossy().to_string(), writable.clone());

            Ok(writable)
        }
    }

    #[test]
    fn test_foo() {
        testutils::audiohash_for_test::RESULT.set(Some(|_| Ok(s(""))));

        let source = testutils::fakesource!(
            json = r#"{
                "list": [{"uri": "1.wav", "name": "1.wav"}, {"uri": "2.wav", "name": "2.wav"}],
                "stream": {"1.wav": [1,-1,1], "2.wav": [-2,2,-2,2]}
            }"#
        );

        let samples = source.list().unwrap();

        let s1 = samples.first().unwrap();
        let s2 = samples.get(1).unwrap();

        let mut set = SampleSet::BaseSampleSet(BaseSampleSet::new(s("Favorites")));

        set.add(&source, s1.clone()).unwrap();
        set.add(&source, s2.clone()).unwrap();

        let mut job = ExportJob {
            io: MockIO {
                writable: HashMap::new(),
            },
            target_directory: "/tmp".to_string(),
            conversion: None,
        };

        job.perform(&set, &[&source]).unwrap();

        unsafe {
            let s1_writable = job.io.writable.get("/tmp/1.wav").unwrap().0.borrow();
            let (_, s1_vals, _) = s1_writable.as_slice().align_to::<f32>();
            assert_eq!(s1_vals, &[1.0, -1.0, 1.0]);

            let s2_writable = job.io.writable.get("/tmp/2.wav").unwrap().0.borrow();
            let (_, s2_vals, _) = s2_writable.as_slice().align_to::<f32>();
            assert_eq!(s2_vals, &[-2.0, 2.0, -2.0, 2.0]);
        }
    }
}
