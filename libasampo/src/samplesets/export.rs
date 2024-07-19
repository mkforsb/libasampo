// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use std::{
    collections::HashMap,
    fs::File,
    io::{BufWriter, Seek, Write},
    path::Path,
};

use rayon::prelude::*;
use rayon_progress::ProgressAdaptor;
use uuid::Uuid;

use crate::{
    convert::{convert, decode, ChannelMapping, RateConversion},
    errors::Error,
    prelude::{SampleOps, SourceOps},
    samplesets::{SampleSet, SampleSetOps},
    sources::Source,
};

pub trait IO: Clone + Send + Sync {
    type Writable: 'static + Write + Seek;

    fn create_dirs(&self, path: &Path) -> Result<(), Error>;
    fn create_file(&self, path: &Path) -> Result<Self::Writable, Error>;
}

#[derive(Debug, Clone)]
pub struct DefaultIO;

impl IO for DefaultIO {
    type Writable = File;

    fn create_dirs(&self, path: &Path) -> Result<(), Error> {
        Ok(std::fs::create_dir_all(path)?)
    }

    fn create_file(&self, path: &Path) -> Result<Self::Writable, Error> {
        Ok(File::create(path)?)
    }
}

#[derive(Debug, Clone)]
pub enum WavSampleFormat {
    Float,
    Int,
}

impl From<WavSampleFormat> for hound::SampleFormat {
    fn from(value: WavSampleFormat) -> Self {
        match value {
            WavSampleFormat::Float => hound::SampleFormat::Float,
            WavSampleFormat::Int => hound::SampleFormat::Int,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WavSpec {
    pub channels: u16,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    pub sample_format: WavSampleFormat,
}

impl From<WavSpec> for hound::WavSpec {
    fn from(value: WavSpec) -> Self {
        let WavSpec {
            channels,
            sample_rate,
            bits_per_sample,
            sample_format,
        } = value;
        hound::WavSpec {
            channels,
            sample_rate,
            bits_per_sample,
            sample_format: sample_format.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum RateConversionQuality {
    Fastest,
    Low,
    Medium,
    High,
}

impl From<RateConversionQuality> for samplerate::ConverterType {
    fn from(value: RateConversionQuality) -> Self {
        match value {
            RateConversionQuality::Fastest => samplerate::ConverterType::Linear,
            RateConversionQuality::Low => samplerate::ConverterType::SincFastest,
            RateConversionQuality::Medium => samplerate::ConverterType::SincMediumQuality,
            RateConversionQuality::High => samplerate::ConverterType::SincBestQuality,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Conversion {
    Wav(WavSpec, Option<RateConversionQuality>),
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

#[derive(Debug, Clone)]
pub enum ExportJobMessage {
    ItemsCompleted(usize),
    Error(Error),
    Finished,
}

impl<T> ExportJob<T>
where
    T: IO + 'static,
{
    pub fn perform(
        &self,
        sampleset: &SampleSet,
        sources: &HashMap<Uuid, Source>,
        tx: Option<std::sync::mpsc::Sender<ExportJobMessage>>,
    ) {
        macro_rules! send_or_log {
            ($tx:ident, $msg:expr) => {{
                let _ = $tx.send($msg).inspect_err(|e| {
                    log::log!(log::Level::Error, "Failed send on channel: {e}");
                });
            }};
        }

        let target_path = Path::new(&self.target_directory);

        match self
            .io
            .create_dirs(target_path)
            .map_err(|e| Error::IoError {
                uri: target_path.to_string_lossy().to_string(),
                details: e.to_string(),
            }) {
            Ok(_) => (),
            Err(e) => {
                if let Some(tx) = tx {
                    send_or_log!(tx, ExportJobMessage::Error(e))
                }
                return;
            }
        }

        let job_copy = self.clone();
        let sources_copy = sources.clone();
        let target_dir_copy = self.target_directory.clone();

        let samplelist = sampleset.list().into_iter().cloned().collect::<Vec<_>>();

        let it = ProgressAdaptor::new(samplelist);
        let progress = it.items_processed();

        let (rayon_tx, rayon_rx) = std::sync::mpsc::channel::<Error>();

        rayon::spawn(move || {
            let result = it.try_for_each(|sample| -> Result<(), Error> {
                let source_uuid =
                    sample
                        .source_uuid()
                        .ok_or(Error::SampleMissingSourceUUIDError(
                            sample.uri().to_string(),
                        ))?;

                let mut filename = Path::new(&target_dir_copy).to_path_buf();

                match &job_copy.conversion {
                    Some(Conversion::Wav(..)) => filename.push(format!("{}.wav", sample.name())),
                    None => filename.push(sample.name()),
                }

                let mut dst = job_copy
                    .io
                    .create_file(&filename)
                    .map_err(|e| Error::IoError {
                        uri: filename.to_string_lossy().to_string(),
                        details: e.to_string(),
                    })?;

                match &job_copy.conversion {
                    Some(Conversion::Wav(spec, rcq)) => {
                        let spec: hound::WavSpec = spec.clone().into();
                        let rcq: Option<samplerate::ConverterType> = rcq.clone().map(|x| x.into());

                        let channel_delta: i32 =
                            spec.channels as i32 - sample.metadata().channels as i32;

                        let chanmap =
                            match (channel_delta, sample.metadata().channels, spec.channels) {
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
                            sources_copy
                                .get(source_uuid)
                                .ok_or(Error::MissingSourceError(*source_uuid))?,
                            &sample,
                        )?;

                        let mut writer = hound::WavWriter::new(BufWriter::new(dst), spec)
                            .map_err(|e| Error::WavEncoderError(e.to_string()))?;

                        let in_channels = sample.metadata().channels;

                        match &spec.sample_format {
                            hound::SampleFormat::Float => {
                                convert::<f32>(samples, in_channels, chanmap, rateconv, rcq)?
                                    .into_iter()
                                    .map(|s| writer.write_sample(s))
                                    .consume()
                            }

                            hound::SampleFormat::Int => match spec.bits_per_sample {
                                32 => convert::<i32>(samples, in_channels, chanmap, rateconv, rcq)?
                                    .into_iter()
                                    .map(|s| writer.write_sample(s))
                                    .consume(),
                                16 => convert::<i16>(samples, in_channels, chanmap, rateconv, rcq)?
                                    .into_iter()
                                    .map(|s| writer.write_sample(s))
                                    .consume(),
                                8 => convert::<i8>(samples, in_channels, chanmap, rateconv, rcq)?
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

                        writer.finalize().map_err(|e| Error::IoError {
                            uri: sample.uri().to_string(),
                            details: e.to_string(),
                        })?;
                    }

                    None => {
                        sources_copy
                            .get(source_uuid)
                            .ok_or(Error::MissingSourceError(*source_uuid))?
                            .raw_copy(&sample, &mut dst)?;
                    }
                }
                Ok(())
            });

            match result {
                Ok(_) => (),
                Err(e) => {
                    send_or_log!(rayon_tx, e);
                }
            }
        });

        let mut prev_completed = 0;

        loop {
            let completed = progress.get();

            if completed != prev_completed {
                prev_completed = completed;

                if let Some(tx) = tx.as_ref() {
                    send_or_log!(tx, ExportJobMessage::ItemsCompleted(completed))
                }
            }

            if completed >= sampleset.len() {
                break;
            }

            match rayon_rx.try_recv() {
                Ok(err) => {
                    if let Some(tx) = tx {
                        send_or_log!(
                            tx,
                            ExportJobMessage::Error(Error::ExportError(Box::new(err)))
                        )
                    }
                    return;
                }

                Err(e) => match e {
                    std::sync::mpsc::TryRecvError::Empty => (),
                    std::sync::mpsc::TryRecvError::Disconnected => break,
                },
            }
        }

        if let Some(tx) = tx {
            send_or_log!(tx, ExportJobMessage::Finished)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use crate::{
        samplesets::BaseSampleSet,
        testutils::{self, s},
    };

    use super::*;

    #[derive(Debug, Clone)]
    struct MockIOWritable(Arc<Mutex<Vec<u8>>>);

    #[derive(Debug, Clone)]
    struct MockIO {
        pub writable: Arc<Mutex<HashMap<String, MockIOWritable>>>,
    }

    impl std::io::Write for MockIOWritable {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        Error::IoError {
                            uri: "???".to_string(),
                            details: e.to_string(),
                        },
                    )
                })?
                .extend_from_slice(buf);
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

        fn create_dirs(&self, _path: &Path) -> Result<(), Error> {
            Ok(())
        }

        fn create_file(&self, path: &Path) -> Result<Self::Writable, Error> {
            let writable = MockIOWritable(Arc::new(Mutex::new(Vec::new())));

            self.writable
                .lock()
                .unwrap()
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

        let job = ExportJob {
            io: MockIO {
                writable: Arc::new(Mutex::new(HashMap::new())),
            },
            target_directory: "/tmp".to_string(),
            conversion: None,
        };

        job.perform(
            &set,
            &vec![(*source.uuid(), source)].into_iter().collect(),
            None,
        );

        unsafe {
            {
                let s1_writable = job.io.writable.try_lock().unwrap();
                let s1_writable = s1_writable.get("/tmp/1.wav").unwrap().0.try_lock().unwrap();
                let (_, s1_vals, _) = s1_writable.as_slice().align_to::<f32>();
                assert_eq!(s1_vals, &[1.0, -1.0, 1.0]);
            }

            {
                let s2_writable = job.io.writable.try_lock().unwrap();
                let s2_writable = s2_writable.get("/tmp/2.wav").unwrap().0.try_lock().unwrap();
                let (_, s2_vals, _) = s2_writable.as_slice().align_to::<f32>();
                assert_eq!(s2_vals, &[-2.0, 2.0, -2.0, 2.0]);
            }
        }
    }
}
