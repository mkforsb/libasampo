// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use std::{
    collections::HashMap,
    fs::File,
    io::{BufWriter, Seek, Write},
    path::Path,
};

use uuid::Uuid;

use crate::{
    convert::{convert, decode, ChannelMapping, RateConversion},
    errors::Error,
    prelude::{SampleOps, SourceOps},
    samplesets::{SampleSet, SampleSetOps},
    sources::Source,
};

pub trait IO {
    type Writable: 'static + Write + Seek;

    fn create_dir_all(&mut self, path: &Path) -> Result<(), Error>;
    fn file_create(&mut self, path: &Path) -> Result<Self::Writable, Error>;
}

pub struct DefaultIO;

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
    Wav(hound::WavSpec, Option<samplerate::ConverterType>),
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

/// A trait for lazy iterators that can consume their elements without forming a
/// collection. Useful for iterators that produce side-effects.
trait Consume<I>
where
    I: Iterator,
{
    /// Consume and discard all elements.
    fn consume(self);
}

impl<T> Consume<T> for T
where
    T: Iterator,
{
    fn consume(self) {
        for _x in self {}
    }
}

impl<T> ExportJob<T>
where
    T: IO,
{
    pub fn perform(
        &mut self,
        sampleset: &SampleSet,
        sources: &HashMap<Uuid, Source>,
    ) -> Result<(), Error> {
        let target_path = Path::new(&self.target_directory);

        self.io
            .create_dir_all(target_path)
            .map_err(|e| Error::IoError {
                uri: target_path.to_string_lossy().to_string(),
                details: e.to_string(),
            })?;

        for sample in sampleset.list().iter() {
            let source_uuid = sample
                .source_uuid()
                .ok_or(Error::SampleMissingSourceUUIDError(
                    sample.uri().to_string(),
                ))?;

            let mut filename = target_path.to_path_buf();

            match &self.conversion {
                Some(Conversion::Wav(..)) => filename.push(format!("{}.wav", sample.name())),
                None => filename.push(sample.name()),
            }

            let mut dst = self.io.file_create(&filename).map_err(|e| Error::IoError {
                uri: filename.to_string_lossy().to_string(),
                details: e.to_string(),
            })?;

            match &self.conversion {
                Some(Conversion::Wav(spec, rcq)) => {
                    let channel_delta: i32 =
                        spec.channels as i32 - sample.metadata().channels as i32;

                    let chanmap = match (channel_delta, sample.metadata().channels, spec.channels) {
                        (0, _, _) => Ok(ChannelMapping::Passthrough),
                        (_, 1, 2) => Ok(ChannelMapping::MonoToStereo),
                        (_, 2, 1) => Ok(ChannelMapping::StereoToMono),
                        _ => Err(Error::SampleConversionError(
                            "Unsupported channel mapping".to_string(),
                        )),
                    }?;

                    let rateconv = if sample.metadata().rate != spec.sample_rate {
                        Some(RateConversion {
                            from: sample.metadata().rate,
                            to: spec.sample_rate,
                        })
                    } else {
                        None
                    };

                    let samples = decode(
                        sources
                            .get(source_uuid)
                            .ok_or(Error::MissingSourceError(*source_uuid))?,
                        sample,
                    )?;

                    let mut writer = hound::WavWriter::new(BufWriter::new(dst), *spec)
                        .map_err(|e| Error::WavEncoderError(e.to_string()))?;

                    let in_channels = sample.metadata().channels;

                    match &spec.sample_format {
                        hound::SampleFormat::Float => {
                            convert::<f32>(samples, in_channels, chanmap, rateconv, *rcq)?
                                .into_iter()
                                .map(|s| writer.write_sample(s))
                                .consume()
                        }

                        hound::SampleFormat::Int => match spec.bits_per_sample {
                            32 => convert::<i32>(samples, in_channels, chanmap, rateconv, *rcq)?
                                .into_iter()
                                .map(|s| writer.write_sample(s))
                                .consume(),
                            16 => convert::<i16>(samples, in_channels, chanmap, rateconv, *rcq)?
                                .into_iter()
                                .map(|s| writer.write_sample(s))
                                .consume(),
                            8 => convert::<i8>(samples, in_channels, chanmap, rateconv, *rcq)?
                                .into_iter()
                                .map(|s| writer.write_sample(s))
                                .consume(),
                            _ => {
                                return Err(Error::SampleConversionError(
                                    "Unsupported bit depth".to_string(),
                                ))
                            }
                        },
                    }

                    writer.finalize().unwrap();
                }

                None => {
                    sources
                        .get(source_uuid)
                        .ok_or(Error::MissingSourceError(*source_uuid))?
                        .raw_copy(sample, &mut dst)?;
                }
            }
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

    impl std::io::Seek for MockIOWritable {
        fn seek(&mut self, _pos: std::io::SeekFrom) -> std::io::Result<u64> {
            unimplemented!()
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
    fn test_plain_copy() {
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

        job.perform(&set, &vec![(*source.uuid(), source)].into_iter().collect())
            .unwrap();

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
