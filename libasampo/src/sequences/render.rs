// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use std::{cmp::Ordering, collections::HashMap};

use crate::{
    convert::{convert, decode, ChannelMapping, RateConversion},
    prelude::{
        ConcreteSampleSetLabelling, SampleOps, SampleSetLabellingOps, SampleSetOps, SourceOps,
        StepSequenceOps,
    },
    samples::SampleMetadata,
    samplesets::{DrumkitLabel, SampleSet, SampleSetLabelling},
    sequences::{DrumkitSequence, Samplerate},
    sources::Source,
};

pub trait DrumkitSampleLoader {
    fn load_sample(&self, label_to_load: &DrumkitLabel) -> Option<(SampleMetadata, Vec<f32>)>;
    fn labels(&self) -> Vec<DrumkitLabel>;
}

#[derive(Debug, Clone)]
pub struct SampleSetSampleLoader {
    sample_set: SampleSet,
    sources: Vec<Source>,
}

impl DrumkitSampleLoader for SampleSetSampleLoader {
    fn load_sample(&self, label_to_load: &DrumkitLabel) -> Option<(SampleMetadata, Vec<f32>)> {
        match self.sample_set.labelling() {
            Some(SampleSetLabelling::DrumkitLabelling(labelling)) if !labelling.is_empty() => self
                .sample_set
                .list()
                .iter()
                .find(|sample| {
                    labelling
                        .get(sample.uri())
                        .is_some_and(|sample_label| sample_label == label_to_load)
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
                }),
            Some(SampleSetLabelling::DrumkitLabelling(_)) | None => None,
        }
    }

    fn labels(&self) -> Vec<DrumkitLabel> {
        match self.sample_set.labelling() {
            Some(SampleSetLabelling::DrumkitLabelling(labelling)) if !labelling.is_empty() => self
                .sample_set
                .list()
                .iter()
                .filter_map(|s| labelling.get(s.uri()).cloned())
                .collect(),
            Some(SampleSetLabelling::DrumkitLabelling(_)) | None => vec![],
        }
    }
}

mod dksrender {
    use std::{
        rc::Rc,
        sync::mpsc::{channel, Receiver, TryRecvError},
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
    struct ActiveSound {
        label: DrumkitLabel,
        samples_generation: usize,
        amplitude: f32,
        offset_in_frames: usize,
        num_frames: usize,
    }

    #[derive(Debug, Clone)]
    struct LoadedSequence {
        sequence: DrumkitSequence,
        step_frames_remain: f64,
        active_sounds: Vec<ActiveSound>,
        mixbuffer_cap: usize,
    }

    #[derive(Debug, Clone)]
    pub struct DrumkitSequenceRenderer {
        sequence: DrumkitSequence,
        output_samplerate: Samplerate,
        samples: Vec<HashMap<DrumkitLabel, Vec<f32>>>,
        samples_current_generation: usize,
        sample_loaders: Vec<ThreadedPromise<HashMap<DrumkitLabel, Vec<f32>>>>,
        current_step: usize,
        step_frames_remain: f64,
        active_sounds: Vec<ActiveSound>,
        mixbuffer: Vec<f32>,
    }

    impl DrumkitSequenceRenderer {
        pub fn new(
            seq: DrumkitSequence,
            output_samplerate: Samplerate,
            sample_loader: Option<&impl DrumkitSampleLoader>,
        ) -> Self {
            let mut samples: HashMap<DrumkitLabel, Vec<f32>> = HashMap::new();

            if let Some(loader) = sample_loader {
                for label in loader.labels() {
                    let (metadata, audio_data) = loader.load_sample(&label).unwrap();

                    samples.insert(
                        label,
                        to_stereo_with_samplerate(audio_data, metadata, output_samplerate.get()),
                    );
                }
            }

            let loaded_seq = Self::load_sequence(seq, output_samplerate, &samples, 0);

            Self {
                sequence: loaded_seq.sequence,
                output_samplerate,
                samples: vec![samples],
                samples_current_generation: 0,
                sample_loaders: vec![],
                current_step: 0,
                step_frames_remain: loaded_seq.step_frames_remain,
                active_sounds: loaded_seq.active_sounds,
                mixbuffer: vec![0.0f32; loaded_seq.mixbuffer_cap],
            }
        }

        pub fn render(&mut self, buffer: &mut [f32]) -> usize {
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

            // TODO: remove unused sample cache generations

            let mut frames_to_write = buffer.len() / 2;
            let mut output_buffer_offset = 0;

            while frames_to_write > 0 {
                let frames_this_cycle =
                    std::cmp::min(frames_to_write, self.step_frames_remain as usize);

                // zero mixbuffer
                self.mixbuffer[..(frames_this_cycle * 2)].fill(0.0);

                // mix active sounds into mixbuffer
                self.active_sounds.iter_mut().for_each(|s| {
                    let frames =
                        std::cmp::min(frames_this_cycle, s.num_frames - s.offset_in_frames);

                    self.mixbuffer[..(frames * 2)]
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
                    .copy_from_slice(&self.mixbuffer[..(frames_this_cycle * 2)]);

                output_buffer_offset += frames_this_cycle * 2;

                self.step_frames_remain -= frames_this_cycle as f64;
                frames_to_write -= frames_this_cycle;

                if self.step_frames_remain < 1.0 {
                    // fetch next step and add active sounds
                    self.current_step = (self.current_step + 1) % self.sequence.len();

                    if let Some(step) = self
                        .sequence
                        .step(self.current_step, self.output_samplerate)
                    {
                        self.step_frames_remain += step.length_in_samples;

                        step.triggers
                            .iter()
                            .filter(|t| {
                                self.samples[self.samples_current_generation].contains_key(&t.label)
                            })
                            .for_each(|t| {
                                self.active_sounds.push(ActiveSound {
                                    label: t.label.clone(),
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
                }
            }

            buffer.len()
        }

        fn load_sequence(
            seq: DrumkitSequence,
            output_samplerate: Samplerate,
            samples: &HashMap<DrumkitLabel, Vec<f32>>,
            samples_generation: usize,
        ) -> LoadedSequence {
            let step0 = seq.step(0, output_samplerate).unwrap();
            let step_frames_remain = step0.length_in_samples;

            let active_sounds = step0
                .triggers
                .iter()
                .filter_map(|trigger| {
                    samples.get(&trigger.label).map(|sampledata| ActiveSound {
                        label: trigger.label.clone(),
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

            LoadedSequence {
                sequence: seq,
                step_frames_remain,
                active_sounds,
                mixbuffer_cap: mixbuffer_cap as usize,
            }
        }

        pub fn set_sequence(&mut self, sequence: DrumkitSequence) {
            let loaded_seq = Self::load_sequence(
                sequence,
                self.output_samplerate,
                &self.samples[self.samples_current_generation],
                self.samples_current_generation,
            );

            self.sequence = loaded_seq.sequence;
            self.current_step = 0;
            self.step_frames_remain = loaded_seq.step_frames_remain;
            self.active_sounds = loaded_seq.active_sounds;
            self.mixbuffer = vec![0.0f32; loaded_seq.mixbuffer_cap];
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

        pub fn set_samples(&mut self, loader: impl DrumkitSampleLoader + Send + 'static) {
            let samplerate = self.output_samplerate.get();

            self.sample_loaders
                .push(ThreadedPromise::<HashMap<DrumkitLabel, Vec<f32>>>::new(
                    move || {
                        let mut result = HashMap::<DrumkitLabel, Vec<f32>>::new();

                        for label in loader.labels() {
                            let (metadata, audio_data) = loader.load_sample(&label).unwrap();

                            result.insert(
                                label,
                                to_stereo_with_samplerate(audio_data, metadata, samplerate),
                            );
                        }

                        result
                    },
                ));
        }
    }
}

pub use dksrender::DrumkitSequenceRenderer;

#[cfg(test)]
mod tests {
    use std::env;

    use crate::{
        samplesets::{BaseSampleSet, DrumkitLabelling},
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

        let mut labels = DrumkitLabelling::new();

        labels.set(bd.uri().clone(), DrumkitLabel::BassDrum);
        labels.set(ch.uri().clone(), DrumkitLabel::ClosedHihat);
        labels.set(sd.uri().clone(), DrumkitLabel::Snare);

        set.set_labelling(Some(SampleSetLabelling::DrumkitLabelling(labels)));

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

        seq.set_step_trigger(4, DrumkitLabel::Snare, 0.5);
        seq.set_step_trigger(12, DrumkitLabel::Snare, 0.5);

        seq
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
        let mut renderer = DrumkitSequenceRenderer::new(
            basic_beat(),
            44100.try_into().unwrap(),
            Some(&drumkit_loader()),
        );

        let mut resultbuf = vec![0.0f32; 2 * 4 * 44100];

        assert_eq!(renderer.render(resultbuf.as_mut_slice()), resultbuf.len());

        write_wav_f32(
            &format!("{}/basic_beat.wav", env::var("CARGO_MANIFEST_DIR").unwrap()),
            &vec![resultbuf],
            44100,
        );
    }

    #[cfg_attr(not(feature = "wav-output-tests"), ignore)]
    #[test]
    fn test_wav_basic_beat_bpm_swing_changes() {
        let mut renderer = DrumkitSequenceRenderer::new(
            basic_beat(),
            44100.try_into().unwrap(),
            Some(&drumkit_loader()),
        );

        let mut buf1 = vec![0.0f32; 2 * 44100];
        let mut buf2 = vec![0.0f32; 2 * 44100];
        let mut buf3 = vec![0.0f32; 2 * 44100];
        let mut buf4 = vec![0.0f32; 2 * 44100];

        assert_eq!(renderer.render(buf1.as_mut_slice()), buf1.len());

        renderer.set_tempo(BPM::new(130).unwrap());
        renderer.set_swing(Swing::new(0.33).unwrap());
        assert_eq!(renderer.render(buf2.as_mut_slice()), buf2.len());

        renderer.set_tempo(BPM::new(160).unwrap());
        renderer.set_swing(Swing::new(0.0).unwrap());
        assert_eq!(renderer.render(buf3.as_mut_slice()), buf3.len());

        renderer.set_tempo(BPM::new(60).unwrap());
        assert_eq!(renderer.render(buf4.as_mut_slice()), buf4.len());

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
        let mut renderer = DrumkitSequenceRenderer::new(
            basic_beat(),
            44100.try_into().unwrap(),
            Some(&drumkit_loader()),
        );

        let mut buf1 = vec![0.0f32; 2 * 2 * 44100];
        let mut buf2 = vec![0.0f32; 2 * 2 * 44100];
        let mut buf3 = vec![0.0f32; 2 * 2 * 44100];
        let mut buf4 = vec![0.0f32; 2 * 2 * 44100];

        assert_eq!(renderer.render(buf1.as_mut_slice()), buf1.len());

        renderer.sequence_set_step_trigger(2, DrumkitLabel::BassDrum, 0.5);
        renderer.sequence_set_step_trigger(5, DrumkitLabel::Snare, 0.5);
        assert_eq!(renderer.render(buf2.as_mut_slice()), buf2.len());

        renderer.sequence_unset_step_trigger(0, DrumkitLabel::BassDrum);
        renderer.sequence_unset_step_trigger(4, DrumkitLabel::BassDrum);
        renderer.sequence_unset_step_trigger(8, DrumkitLabel::BassDrum);
        assert_eq!(renderer.render(buf3.as_mut_slice()), buf3.len());

        renderer.sequence_clear();
        assert_eq!(renderer.render(buf4.as_mut_slice()), buf4.len());

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
        let mut renderer = DrumkitSequenceRenderer::new(
            basic_beat(),
            44100.try_into().unwrap(),
            Some(&drumkit_loader()),
        );

        let mut buf1 = vec![0.0f32; 2 * 2 * 44100];
        let mut buf2 = vec![0.0f32; 2 * 2 * 44100];

        let (source, mut set) = drumkit();

        let bd_uri = set
            .list()
            .iter()
            .find(|s| s.name() == "kick.wav")
            .unwrap()
            .uri()
            .clone();
        let ch_uri = set
            .list()
            .iter()
            .find(|s| s.name() == "hihat.wav")
            .unwrap()
            .uri()
            .clone();
        let sd_uri = set
            .list()
            .iter()
            .find(|s| s.name() == "snare.wav")
            .unwrap()
            .uri()
            .clone();

        if let Some(SampleSetLabelling::DrumkitLabelling(labels)) = set.labelling_mut() {
            labels.set(bd_uri, DrumkitLabel::Snare);
            labels.set(ch_uri, DrumkitLabel::BassDrum);
            labels.set(sd_uri, DrumkitLabel::ClosedHihat);
        }

        assert_eq!(renderer.render(buf1.as_mut_slice()), buf1.len());

        renderer.set_samples(SampleSetSampleLoader {
            sample_set: set,
            sources: vec![source],
        });

        std::thread::sleep(std::time::Duration::from_millis(100));

        assert_eq!(renderer.render(buf2.as_mut_slice()), buf2.len());

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
