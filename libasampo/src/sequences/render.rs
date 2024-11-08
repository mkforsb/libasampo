// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use std::{cmp::Ordering, collections::HashMap};

use crate::{
    convert::{convert, decode, ChannelMapping, RateConversion},
    prelude::{SampleOps, SampleSetOps, SourceOps, StepSequenceOps},
    samples::SampleMetadata,
    samplesets::{DrumkitLabel, SampleSet},
    sequences::{DrumkitSequence, Samplerate},
    sources::Source,
};

pub trait DrumkitSampleLoader {
    fn load_sample(&self, label_to_load: DrumkitLabel) -> Option<(SampleMetadata, Vec<f32>)>;
    fn labels(&self) -> Vec<DrumkitLabel>;
}

#[derive(Debug, Clone)]
pub struct SampleSetSampleLoader {
    sample_set: SampleSet,
    sources: Vec<Source>,
}

impl SampleSetSampleLoader {
    pub fn new(sample_set: SampleSet, sources: Vec<Source>) -> Self {
        Self {
            sample_set,
            sources,
        }
    }
}

impl DrumkitSampleLoader for SampleSetSampleLoader {
    fn load_sample(&self, label_to_load: DrumkitLabel) -> Option<(SampleMetadata, Vec<f32>)> {
        self.sample_set
            .list()
            .iter()
            .find(|sample| {
                self.sample_set
                    .get_label::<DrumkitLabel>(sample)
                    .is_ok_and(|sample_label| sample_label == Some(label_to_load))
            })
            .and_then(|sample| {
                self.sources
                    .iter()
                    .find(|source| {
                        source.uuid()
                            == sample
                                .source_uuid()
                                .expect("Loadable samples should have a source UUID")
                    })
                    .and_then(|source| {
                        Some((sample.metadata().clone(), decode(source, sample).ok()?))
                    })
            })
    }

    fn labels(&self) -> Vec<DrumkitLabel> {
        self.sample_set
            .list()
            .iter()
            .filter_map(|s| {
                self.sample_set
                    .get_label::<DrumkitLabel>(s)
                    .map_or(None, |x| x)
            })
            .collect()
    }
}

mod dksrender {
    use std::{
        rc::Rc,
        sync::mpsc::{channel, Receiver, TryRecvError},
        time::{Duration, Instant},
    };

    use crate::sequences::{time::Swing, TimeSpec, BPM};

    use super::*;

    #[derive(Debug, Clone)]
    enum ThreadedPromiseState<T: Send + 'static> {
        Pending,
        Ready(T),
        Failed,
    }

    #[derive(Debug, Clone)]
    struct ThreadedPromise<T: Send + 'static> {
        rx: Rc<Receiver<T>>,
    }

    impl<T> ThreadedPromise<T>
    where
        T: Send + 'static,
    {
        pub fn new(func: impl (FnOnce() -> T) + Send + 'static) -> Self {
            let (tx, rx) = channel::<T>();

            let _ = std::thread::spawn(move || tx.send(func()));

            Self { rx: Rc::new(rx) }
        }

        pub fn poll(&self) -> ThreadedPromiseState<T> {
            match self.rx.try_recv() {
                Ok(value) => ThreadedPromiseState::<T>::Ready(value),
                Err(e) => match e {
                    TryRecvError::Empty => ThreadedPromiseState::<T>::Pending,
                    TryRecvError::Disconnected => ThreadedPromiseState::<T>::Failed,
                },
            }
        }
    }

    fn to_stereo_with_samplerate(
        audio_data: Vec<f32>,
        metadata: SampleMetadata,
        target_samplerate: u32,
    ) -> Vec<f32> {
        convert(
            audio_data,
            metadata.channels,
            match metadata.channels {
                1 => ChannelMapping::MonoToStereo,
                2 => ChannelMapping::Passthrough,
                _ => ChannelMapping::TruncateToStereo {
                    input_channels: metadata.channels.try_into().unwrap(),
                },
            },
            match metadata.rate.cmp(&target_samplerate) {
                Ordering::Less | Ordering::Greater => Some(RateConversion {
                    from: metadata.rate,
                    to: target_samplerate,
                }),
                Ordering::Equal => None,
            },
            None,
        )
        .unwrap()
    }

    #[derive(Debug, Clone)]
    pub struct DrumkitSequenceEvent {
        pub labels: Vec<DrumkitLabel>,
        pub step: usize,
        pub time: Instant,
    }

    #[derive(Debug, Clone)]
    struct ActiveSound {
        label: DrumkitLabel,
        samples_generation: usize,
        amplitude: f32,
        offset_in_frames: usize,
        num_frames: usize,
    }

    #[derive(Debug, Clone)]
    struct LoadedSequenceInfo {
        step_frames_remain: f64,
        active_sounds: Vec<ActiveSound>,
        mixbuffer_cap: usize,
    }

    #[derive(Clone)]
    pub struct DrumkitSequenceRenderer {
        sequence: DrumkitSequence,
        output_samplerate: Samplerate,
        samples: Vec<HashMap<DrumkitLabel, Vec<f32>>>,
        samples_current_generation: usize,
        sample_loaders: Vec<ThreadedPromise<HashMap<DrumkitLabel, Vec<f32>>>>,
        current_step: Option<usize>,
        step_frames_remain: Option<f64>,
        active_sounds: Vec<ActiveSound>,
        mixbuffer: Option<Vec<f32>>,
    }

    impl std::fmt::Debug for DrumkitSequenceRenderer {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            if f.alternate() {
                f.write_str(&format!(
                    "DrumkitSequenceRenderer(\n\
                        sequence={:#?}\n,\
                        output_samplerate={:#?},\n\
                        samples={:#?},\n\
                        samples_current_generation={:#?},\n\
                        sample_loaders: {},\n\
                        current_step={:#?},\n\
                        step_frames_remain={:#?}\n\
                        active_sounds={:#?}\n\
                        mixbuffer: {})",
                    self.sequence,
                    self.output_samplerate,
                    self.samples
                        .iter()
                        .map(|hm| hm.keys().collect::<Vec<_>>())
                        .collect::<Vec<_>>(),
                    self.samples_current_generation,
                    self.sample_loaders.len(),
                    self.current_step,
                    self.step_frames_remain,
                    self.active_sounds,
                    match &self.mixbuffer {
                        Some(buf) => format!("{} frames", buf.len() / 8),
                        None => "not initialized".to_string(),
                    }
                ))
            } else {
                f.write_str(&format!(
                    "DrumkitSequenceRenderer(\
                        sequence={:?}, \
                        output_samplerate={:?}, \
                        samples={:?}, \
                        samples_current_generation={:?}, \
                        sample_loaders: {}, \
                        current_step={:?}, \
                        step_frames_remain={:?} \
                        active_sounds={:?} \
                        mixbuffer: {})",
                    self.sequence,
                    self.output_samplerate,
                    self.samples
                        .iter()
                        .map(|hm| hm.keys().collect::<Vec<_>>())
                        .collect::<Vec<_>>(),
                    self.samples_current_generation,
                    self.sample_loaders.len(),
                    self.current_step,
                    self.step_frames_remain,
                    self.active_sounds,
                    match &self.mixbuffer {
                        Some(buf) => format!("{} frames", buf.len() / 8),
                        None => "not initialized".to_string(),
                    }
                ))
            }
        }
    }

    impl DrumkitSequenceRenderer {
        pub fn new(output_samplerate: Samplerate) -> Self {
            Self {
                sequence: DrumkitSequence::default(),
                output_samplerate,
                samples: vec![HashMap::new()],
                samples_current_generation: 0,
                sample_loaders: Vec::new(),
                current_step: None,
                step_frames_remain: None,
                active_sounds: Vec::new(),
                mixbuffer: None,
            }
        }

        pub fn render(&mut self, buffer: &mut [f32]) -> (usize, Option<Vec<DrumkitSequenceEvent>>) {
            let render_start = Instant::now();
            let duration_per_frame =
                Duration::from_secs_f64(1.0f64 / self.output_samplerate.get() as f64);
            let mut events: Vec<DrumkitSequenceEvent> = Vec::new();

            self.check_sample_loaders();

            if self.current_step.is_none() {
                self.init_sequence();
                events.push(DrumkitSequenceEvent {
                    labels: self
                        .active_sounds
                        .iter()
                        .map(|s| s.label)
                        .collect::<Vec<_>>(),
                    step: 0,
                    time: render_start,
                });
            }

            let step_frames_remain = self.step_frames_remain.as_mut().unwrap();
            let current_step = self.current_step.as_mut().unwrap();
            let mixbuffer = self.mixbuffer.as_mut().unwrap();

            // TODO: remove unused sample cache generations

            let mut frames_to_write = buffer.len() / 2;
            let mut output_buffer_offset = 0;
            let mut frames_written = 0;

            while frames_to_write > 0 {
                let frames_this_cycle =
                    std::cmp::min(frames_to_write, *step_frames_remain as usize);

                // zero mixbuffer
                mixbuffer[..(frames_this_cycle * 2)].fill(0.0);

                // mix active sounds into mixbuffer
                self.active_sounds.iter_mut().for_each(|s| {
                    let frames =
                        std::cmp::min(frames_this_cycle, s.num_frames - s.offset_in_frames);

                    mixbuffer[..(frames * 2)]
                        .iter_mut()
                        .zip(
                            self.samples[s.samples_generation]
                                .get(&s.label)
                                .unwrap()
                                .as_slice()
                                [(s.offset_in_frames * 2)..((s.offset_in_frames + frames) * 2)]
                                .iter(),
                        )
                        .for_each(|(mix, sound)| {
                            *mix += sound * s.amplitude;
                        });

                    s.offset_in_frames += frames;
                });

                // drop finished sounds
                self.active_sounds
                    .retain(|s| s.offset_in_frames < s.num_frames);

                // write mixbuffer into output buffer
                buffer[output_buffer_offset..(output_buffer_offset + (frames_this_cycle * 2))]
                    .copy_from_slice(&mixbuffer[..(frames_this_cycle * 2)]);

                frames_written += frames_this_cycle;
                output_buffer_offset += frames_this_cycle * 2;

                *step_frames_remain -= frames_this_cycle as f64;
                frames_to_write -= frames_this_cycle;

                if *step_frames_remain < 1.0 {
                    // fetch next step and add active sounds
                    *current_step = (*current_step + 1) % self.sequence.len();

                    if let Some(step) = self.sequence.step(*current_step) {
                        *step_frames_remain += step.length_in_samples(self.output_samplerate);

                        step.triggers
                            .iter()
                            .filter(|t| {
                                self.samples[self.samples_current_generation].contains_key(&t.label)
                            })
                            .for_each(|t| {
                                self.active_sounds.push(ActiveSound {
                                    label: t.label,
                                    samples_generation: self.samples_current_generation,
                                    amplitude: t.amplitude,
                                    offset_in_frames: 0,
                                    num_frames: self.samples[self.samples_current_generation]
                                        .get(&t.label)
                                        .unwrap()
                                        .len()
                                        / 2,
                                })
                            });
                    }

                    events.push(DrumkitSequenceEvent {
                        labels: self
                            .active_sounds
                            .iter()
                            .map(|s| s.label)
                            .collect::<Vec<_>>(),
                        step: *current_step,
                        time: render_start
                            + duration_per_frame.saturating_mul(frames_written as u32),
                    });
                }
            }

            (
                buffer.len(),
                if events.is_empty() {
                    None
                } else {
                    Some(events)
                },
            )
        }

        pub fn reset_sequence(&mut self) {
            self.current_step = None;
            self.step_frames_remain = None;
            self.mixbuffer = None;
        }

        pub fn set_sequence(&mut self, sequence: DrumkitSequence) {
            self.sequence = sequence;
            self.reset_sequence();
        }

        pub fn set_tempo(&mut self, bpm: BPM) {
            self.sequence.set_timespec(TimeSpec {
                bpm,
                ..self.sequence.timespec()
            });
        }

        pub fn set_swing(&mut self, swing: Swing) {
            self.sequence.set_timespec(TimeSpec {
                swing,
                ..self.sequence.timespec()
            });
        }

        pub fn sequence_clear(&mut self) {
            self.sequence.clear();
        }

        pub fn sequence_clear_step(&mut self, n: usize) {
            self.sequence.clear_step(n);
        }

        pub fn sequence_set_step_trigger(&mut self, n: usize, label: DrumkitLabel, amp: f32) {
            self.sequence.set_step_trigger(n, label, amp);
        }

        pub fn sequence_unset_step_trigger(&mut self, n: usize, label: DrumkitLabel) {
            self.sequence.unset_step_trigger(n, label);
        }

        pub fn load_samples(&mut self, loader: impl DrumkitSampleLoader) {
            let mut result = HashMap::<DrumkitLabel, Vec<f32>>::new();

            for label in loader.labels() {
                let (metadata, audio_data) = loader.load_sample(label).unwrap();

                result.insert(
                    label,
                    to_stereo_with_samplerate(audio_data, metadata, self.output_samplerate.get()),
                );
            }

            self.samples.push(result);
            self.samples_current_generation += 1;
        }

        pub fn load_samples_async(&mut self, loader: impl DrumkitSampleLoader + Send + 'static) {
            let samplerate = self.output_samplerate.get();

            self.sample_loaders
                .push(ThreadedPromise::<HashMap<DrumkitLabel, Vec<f32>>>::new(
                    move || {
                        let mut result = HashMap::<DrumkitLabel, Vec<f32>>::new();

                        for label in loader.labels() {
                            let (metadata, audio_data) = loader.load_sample(label).unwrap();

                            result.insert(
                                label,
                                to_stereo_with_samplerate(audio_data, metadata, samplerate),
                            );
                        }

                        result
                    },
                ));
        }

        pub fn borrow_sequence(&self) -> &DrumkitSequence {
            &self.sequence
        }

        pub fn output_samplerate(&self) -> Samplerate {
            self.output_samplerate
        }

        fn load_sequence(
            seq: &DrumkitSequence,
            output_samplerate: Samplerate,
            samples: &HashMap<DrumkitLabel, Vec<f32>>,
            samples_generation: usize,
        ) -> LoadedSequenceInfo {
            let step0 = seq.step(0).unwrap();
            let step_frames_remain = step0.length_in_samples(output_samplerate);

            let active_sounds = step0
                .triggers
                .iter()
                .filter_map(|trigger| {
                    samples.get(&trigger.label).map(|sampledata| ActiveSound {
                        label: trigger.label,
                        samples_generation,
                        amplitude: trigger.amplitude,
                        offset_in_frames: 0,
                        num_frames: sampledata.len() / 2,
                    })
                })
                .collect();

            let mixbuffer_cap = 4.0
                * seq
                    .timespec
                    .samples_per_note(output_samplerate, seq.step_base_length);

            assert!(
                mixbuffer_cap <= usize::MAX as f64,
                "Sequence base step length too long"
            );

            LoadedSequenceInfo {
                step_frames_remain,
                active_sounds,
                mixbuffer_cap: mixbuffer_cap as usize,
            }
        }

        fn init_sequence(&mut self) {
            let loaded_seq = Self::load_sequence(
                &self.sequence,
                self.output_samplerate,
                &self.samples[self.samples_current_generation],
                self.samples_current_generation,
            );

            self.current_step = Some(0);
            self.step_frames_remain = Some(loaded_seq.step_frames_remain);
            self.active_sounds = loaded_seq.active_sounds;
            self.mixbuffer = Some(vec![0.0f32; loaded_seq.mixbuffer_cap]);
        }

        fn unload_stale_samples(&mut self) {
            let num_stale_gens = if let Some(earliest_active_generation) = self
                .active_sounds
                .iter()
                .map(|x| x.samples_generation)
                .min()
            {
                earliest_active_generation
            } else {
                self.samples_current_generation
            };

            if num_stale_gens > 0 {
                self.samples.drain(0..num_stale_gens);
                self.samples_current_generation -= num_stale_gens;

                self.active_sounds
                    .iter_mut()
                    .for_each(|s| s.samples_generation -= num_stale_gens);

                log::log!(
                    log::Level::Debug,
                    "Dropped {num_stale_gens} sample generation(s)"
                );
            }
        }

        fn check_sample_loaders(&mut self) {
            let generation_pre = self.samples_current_generation;

            self.sample_loaders
                .retain_mut(|loader| match loader.poll() {
                    ThreadedPromiseState::Pending => true,
                    ThreadedPromiseState::Ready(sample_cache) => {
                        self.samples.push(sample_cache);
                        self.samples_current_generation += 1;
                        false
                    }
                    ThreadedPromiseState::Failed => false,
                });

            if self.samples_current_generation > generation_pre {
                self.unload_stale_samples();
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_load_and_init_sequence() {
            let mut dksr = DrumkitSequenceRenderer::new(44100.try_into().unwrap());

            assert!(dksr.step_frames_remain.is_none());
            assert!(dksr.current_step.is_none());
            assert!(dksr.mixbuffer.is_none());

            let info = DrumkitSequenceRenderer::load_sequence(
                &dksr.sequence,
                dksr.output_samplerate,
                dksr.samples.last().unwrap(),
                dksr.samples_current_generation,
            );

            dksr.init_sequence();

            assert_eq!(dksr.step_frames_remain, Some(info.step_frames_remain));
            assert_eq!(dksr.current_step, Some(0));
            assert_eq!(dksr.mixbuffer.unwrap().len(), info.mixbuffer_cap);
        }

        #[test]
        fn test_unload_stale_samples() {
            fn load_samples(dksr: &mut DrumkitSequenceRenderer) {
                dksr.samples.push(
                    vec![(DrumkitLabel::BassDrum, vec![])]
                        .into_iter()
                        .collect::<HashMap<_, _>>(),
                );

                dksr.samples_current_generation += 1;
            }

            let mut dksr = DrumkitSequenceRenderer::new(44100.try_into().unwrap());

            load_samples(&mut dksr);
            assert_eq!(dksr.samples.len(), 2);

            dksr.active_sounds.push(ActiveSound {
                label: DrumkitLabel::BassDrum,
                samples_generation: dksr.samples_current_generation,
                amplitude: 1.0,
                offset_in_frames: 0,
                num_frames: 1,
            });

            load_samples(&mut dksr);
            assert_eq!(dksr.samples.len(), 3);
            dksr.unload_stale_samples();
            assert_eq!(dksr.samples.len(), 2);

            dksr.active_sounds.clear();
            dksr.unload_stale_samples();
            assert_eq!(dksr.samples.len(), 1);
        }

        /// Test unloading of stale sample generations triggered by async sample loaders.
        #[test]
        fn test_unload_stale_samples_async() {
            fn load_samples(dksr: &mut DrumkitSequenceRenderer) {
                dksr.sample_loaders.push(ThreadedPromise::new(|| {
                    vec![(DrumkitLabel::BassDrum, vec![])]
                        .into_iter()
                        .collect::<HashMap<_, _>>()
                }));

                while !dksr.sample_loaders.is_empty() {
                    dksr.check_sample_loaders();
                }
            }

            let mut dksr = DrumkitSequenceRenderer::new(44100.try_into().unwrap());

            load_samples(&mut dksr);
            assert_eq!(dksr.samples.len(), 1);

            dksr.active_sounds.push(ActiveSound {
                label: DrumkitLabel::BassDrum,
                samples_generation: dksr.samples_current_generation,
                amplitude: 1.0,
                offset_in_frames: 0,
                num_frames: 1,
            });

            load_samples(&mut dksr);
            dksr.unload_stale_samples();
            assert_eq!(dksr.samples.len(), 2);

            dksr.active_sounds.clear();
            dksr.unload_stale_samples();
            assert_eq!(dksr.samples.len(), 1);
        }
    }
}

pub use dksrender::{DrumkitSequenceEvent, DrumkitSequenceRenderer};

#[cfg(test)]
mod tests {
    use std::env;

    use crate::{
        samplesets::BaseSampleSet,
        sequences::{time::Swing, NoteLength, TimeSpec, BPM},
        sources::{file_system_source::FilesystemSource, Source},
    };

    use super::*;

    fn drumkit() -> (Source, SampleSet) {
        let source = Source::FilesystemSource(FilesystemSource::new(
            format!(
                "{}/test_assets/drumkit",
                env::var("CARGO_MANIFEST_DIR").unwrap()
            ),
            vec!["wav".to_string()],
        ));

        let list = source.list().unwrap();

        let bd = list.iter().find(|s| s.name() == "kick.wav").unwrap();
        let ch = list.iter().find(|s| s.name() == "hihat.wav").unwrap();
        let sd = list.iter().find(|s| s.name() == "snare.wav").unwrap();

        let mut set = SampleSet::BaseSampleSet(BaseSampleSet::new("my set".to_string()));

        set.add_with_hash(bd.clone(), "bd".to_string());
        set.add_with_hash(ch.clone(), "ch".to_string());
        set.add_with_hash(sd.clone(), "sd".to_string());

        set.set_label(bd, Some(DrumkitLabel::BassDrum)).unwrap();
        set.set_label(ch, Some(DrumkitLabel::ClosedHihat)).unwrap();
        set.set_label(sd, Some(DrumkitLabel::SnareDrum)).unwrap();

        (source, set)
    }

    fn drumkit_loader() -> SampleSetSampleLoader {
        let (source, set) = drumkit();

        SampleSetSampleLoader {
            sample_set: set,
            sources: vec![source],
        }
    }

    fn basic_beat() -> DrumkitSequence {
        let mut seq = DrumkitSequence::new(
            TimeSpec::new_with_swing(120, 4, 4, 0.0).unwrap(),
            NoteLength::Sixteenth,
        );

        seq.set_step_trigger(0, DrumkitLabel::BassDrum, 0.5);
        seq.set_step_trigger(4, DrumkitLabel::BassDrum, 0.5);
        seq.set_step_trigger(8, DrumkitLabel::BassDrum, 0.5);
        seq.set_step_trigger(12, DrumkitLabel::BassDrum, 0.5);

        for i in 0..16 {
            seq.set_step_trigger(i, DrumkitLabel::ClosedHihat, 0.5);
        }

        seq.set_step_trigger(4, DrumkitLabel::SnareDrum, 0.5);
        seq.set_step_trigger(12, DrumkitLabel::SnareDrum, 0.5);

        seq
    }

    fn renderer(
        samplerate: u32,
        samples: impl DrumkitSampleLoader,
        sequence: DrumkitSequence,
    ) -> DrumkitSequenceRenderer {
        let mut renderer = DrumkitSequenceRenderer::new(samplerate.try_into().unwrap());
        renderer.load_samples(samples);
        renderer.set_sequence(sequence);
        renderer
    }

    fn write_wav_f32(path: &str, bufs: &Vec<Vec<f32>>, sample_rate: u32) {
        let mut writer = hound::WavWriter::create(
            path,
            hound::WavSpec {
                channels: 2,
                sample_rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();

        for buf in bufs {
            for sample in buf {
                let _ = writer.write_sample(*sample);
            }
        }

        let _ = writer.finalize();
    }

    #[cfg_attr(not(feature = "wav-output-tests"), ignore)]
    #[test]
    fn test_wav_basic_beat() {
        let mut renderer = renderer(44100, drumkit_loader(), basic_beat());
        let mut resultbuf = vec![0.0f32; 2 * 4 * 44100];

        assert_eq!(renderer.render(resultbuf.as_mut_slice()).0, resultbuf.len());

        write_wav_f32(
            &format!("{}/basic_beat.wav", env::var("CARGO_MANIFEST_DIR").unwrap()),
            &vec![resultbuf],
            44100,
        );
    }

    #[cfg_attr(not(feature = "wav-output-tests"), ignore)]
    #[test]
    fn test_wav_basic_beat_bpm_swing_changes() {
        let mut renderer = renderer(44100, drumkit_loader(), basic_beat());

        let mut buf1 = vec![0.0f32; 2 * 44100];
        let mut buf2 = vec![0.0f32; 2 * 44100];
        let mut buf3 = vec![0.0f32; 2 * 44100];
        let mut buf4 = vec![0.0f32; 2 * 44100];

        assert_eq!(renderer.render(buf1.as_mut_slice()).0, buf1.len());

        renderer.set_tempo(BPM::new(130).unwrap());
        renderer.set_swing(Swing::new(0.33).unwrap());
        assert_eq!(renderer.render(buf2.as_mut_slice()).0, buf2.len());

        renderer.set_tempo(BPM::new(160).unwrap());
        renderer.set_swing(Swing::new(0.0).unwrap());
        assert_eq!(renderer.render(buf3.as_mut_slice()).0, buf3.len());

        renderer.set_tempo(BPM::new(60).unwrap());
        assert_eq!(renderer.render(buf4.as_mut_slice()).0, buf4.len());

        write_wav_f32(
            &format!(
                "{}/basic_beat_swing_changes.wav",
                env::var("CARGO_MANIFEST_DIR").unwrap()
            ),
            &vec![buf1, buf2, buf3, buf4],
            44100,
        );
    }

    #[cfg_attr(not(feature = "wav-output-tests"), ignore)]
    #[test]
    fn test_wav_basic_beat_step_changes() {
        let mut renderer = renderer(44100, drumkit_loader(), basic_beat());

        let mut buf1 = vec![0.0f32; 2 * 2 * 44100];
        let mut buf2 = vec![0.0f32; 2 * 2 * 44100];
        let mut buf3 = vec![0.0f32; 2 * 2 * 44100];
        let mut buf4 = vec![0.0f32; 2 * 2 * 44100];

        assert_eq!(renderer.render(buf1.as_mut_slice()).0, buf1.len());

        renderer.sequence_set_step_trigger(2, DrumkitLabel::BassDrum, 0.5);
        renderer.sequence_set_step_trigger(5, DrumkitLabel::SnareDrum, 0.5);
        assert_eq!(renderer.render(buf2.as_mut_slice()).0, buf2.len());

        renderer.sequence_unset_step_trigger(0, DrumkitLabel::BassDrum);
        renderer.sequence_unset_step_trigger(4, DrumkitLabel::BassDrum);
        renderer.sequence_unset_step_trigger(8, DrumkitLabel::BassDrum);
        assert_eq!(renderer.render(buf3.as_mut_slice()).0, buf3.len());

        renderer.sequence_clear();
        assert_eq!(renderer.render(buf4.as_mut_slice()).0, buf4.len());

        write_wav_f32(
            &format!(
                "{}/basic_beat_step_changes.wav",
                env::var("CARGO_MANIFEST_DIR").unwrap()
            ),
            &vec![buf1, buf2, buf3, buf4],
            44100,
        );
    }

    #[cfg_attr(not(feature = "wav-output-tests"), ignore)]
    #[test]
    fn test_wav_basic_beat_sample_swap() {
        let mut renderer = renderer(44100, drumkit_loader(), basic_beat());

        let mut buf1 = vec![0.0f32; 2 * 2 * 44100];
        let mut buf2 = vec![0.0f32; 2 * 2 * 44100];

        let (source, mut set) = drumkit();

        macro_rules! member {
            ($set:ident, $name:expr) => {
                $set.list()
                    .iter()
                    .cloned()
                    .cloned()
                    .find(|sample| sample.name() == $name)
                    .unwrap()
            };
        }

        let bd = member!(set, "kick.wav");
        let ch = member!(set, "hihat.wav");
        let sd = member!(set, "snare.wav");

        set.set_label(&bd, Some(DrumkitLabel::SnareDrum)).unwrap();
        set.set_label(&ch, Some(DrumkitLabel::BassDrum)).unwrap();
        set.set_label(&sd, Some(DrumkitLabel::ClosedHihat)).unwrap();

        assert_eq!(renderer.render(buf1.as_mut_slice()).0, buf1.len());

        renderer.load_samples_async(SampleSetSampleLoader {
            sample_set: set,
            sources: vec![source],
        });

        std::thread::sleep(std::time::Duration::from_millis(100));

        assert_eq!(renderer.render(buf2.as_mut_slice()).0, buf2.len());

        write_wav_f32(
            &format!(
                "{}/basic_beat_sample_swap.wav",
                env::var("CARGO_MANIFEST_DIR").unwrap()
            ),
            &vec![buf1, buf2],
            44100,
        );
    }
}
