// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use std::cmp::Ordering;

use crate::{prelude::ConcreteSampleSetLabelling, samplesets::DrumkitLabelling};

mod render;
mod time;

pub use render::{DrumkitSampleLoader, DrumkitSequenceRenderer, SampleSetSampleLoader};
pub use time::{NoteLength, Samplerate, TimeSignature, TimeSpec, BPM};

#[derive(Debug, Clone)]
pub struct Trigger<T: ConcreteSampleSetLabelling> {
    label: T::Label,
    amplitude: f32,
}

#[derive(Debug, Clone)]
pub struct StepInfo<'a, T: ConcreteSampleSetLabelling> {
    length_in_samples: f64,
    triggers: &'a Vec<Trigger<T>>,
}

pub trait StepSequenceOps<T: ConcreteSampleSetLabelling> {
    fn timespec(&self) -> TimeSpec;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool;
    fn step_base_len(&self) -> NoteLength;
    fn step(&self, n: usize, samplerate: Samplerate) -> Option<StepInfo<T>>;
    fn set_timespec(&mut self, spec: TimeSpec);
    fn set_len(&mut self, len: usize);
    fn set_step_base_len(&mut self, len: NoteLength);
    fn clear(&mut self);
    fn clear_step(&mut self, n: usize);
    fn set_step_trigger(&mut self, n: usize, label: T::Label, amp: f32);
    fn unset_step_trigger(&mut self, n: usize, label: T::Label);
}

#[derive(Debug, Clone)]
pub struct DrumkitSequence {
    timespec: TimeSpec,
    step_base_length: NoteLength,
    steps: Vec<Vec<Trigger<DrumkitLabelling>>>,
}

impl DrumkitSequence {
    pub fn new(timespec: TimeSpec, step_base_length: NoteLength) -> Self {
        let len = timespec.signature.upper() as u64
            * (step_base_length.reciprocal() / timespec.signature.lower() as f64) as u64;

        let mut steps = Vec::new();

        for _ in 0..len {
            steps.push(Vec::new());
        }

        DrumkitSequence {
            timespec,
            step_base_length,
            steps,
        }
    }
}

impl Default for DrumkitSequence {
    fn default() -> Self {
        DrumkitSequence {
            timespec: TimeSpec::new(120, 4, 4).unwrap(),
            step_base_length: NoteLength::Sixteenth,
            steps: vec![Vec::new(); 16],
        }
    }
}

impl StepSequenceOps<DrumkitLabelling> for DrumkitSequence {
    fn timespec(&self) -> TimeSpec {
        self.timespec
    }

    fn len(&self) -> usize {
        self.steps.len()
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn step_base_len(&self) -> NoteLength {
        self.step_base_length
    }

    fn step(&self, n: usize, samplerate: Samplerate) -> Option<StepInfo<DrumkitLabelling>> {
        if let Some(triggers) = self.steps.get(n) {
            let base_len_in_samples = self
                .timespec
                .samples_per_note(samplerate, self.step_base_length);

            let sign = if n % 2 == 0 { 1.0 } else { -1.0 };

            Some(StepInfo {
                length_in_samples: base_len_in_samples * (1.0 + (sign * self.timespec.swing.get())),
                triggers,
            })
        } else {
            None
        }
    }

    fn set_timespec(&mut self, spec: TimeSpec) {
        self.timespec = spec
    }

    fn set_len(&mut self, len: usize) {
        if len > 0 {
            match len.cmp(&self.steps.len()) {
                Ordering::Less => self.steps.truncate(len),
                Ordering::Greater => {
                    for _ in 0..(len - self.steps.len()) {
                        self.steps.push(Vec::new());
                    }
                }
                Ordering::Equal => (),
            }
        } else {
            log::log!(log::Level::Warn, "Attempt to set sequence length to zero");
        }
    }

    fn set_step_base_len(&mut self, len: NoteLength) {
        self.step_base_length = len
    }

    fn clear(&mut self) {
        self.steps.iter_mut().map(|v| v.clear()).count();
    }

    fn clear_step(&mut self, n: usize) {
        if let Some(v) = self.steps.get_mut(n) {
            v.clear();
        }
    }

    fn set_step_trigger(
        &mut self,
        n: usize,
        label: <DrumkitLabelling as ConcreteSampleSetLabelling>::Label,
        amp: f32,
    ) {
        if let Some(v) = self.steps.get_mut(n) {
            v.retain(|trigger| trigger.label != label);
            v.push(Trigger {
                label,
                amplitude: amp,
            });
        }
    }

    fn unset_step_trigger(
        &mut self,
        n: usize,
        label: <DrumkitLabelling as ConcreteSampleSetLabelling>::Label,
    ) {
        if let Some(v) = self.steps.get_mut(n) {
            v.retain(|trigger| trigger.label != label);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use crate::samplesets::DrumkitLabel;

    use super::*;

    fn f64_eq(a: f64, b: f64) -> bool {
        (a - b).abs() < 0.0001
    }

    fn timespec(bpm: u16, sig_upper: u8, sig_lower: u8) -> TimeSpec {
        TimeSpec::new(bpm, sig_upper, sig_lower).unwrap()
    }

    #[test]
    fn test_drumkit_seq_default_length() {
        assert_eq!(
            DrumkitSequence::new(timespec(120, 4, 4), NoteLength::Sixteenth).len(),
            16
        );

        assert_eq!(
            DrumkitSequence::new(timespec(120, 7, 8), NoteLength::Sixteenth).len(),
            14
        );

        assert_eq!(
            DrumkitSequence::new(timespec(140, 3, 4), NoteLength::Sixteenth).len(),
            12
        );
    }

    fn drumkitseq1() -> DrumkitSequence {
        let mut seq = DrumkitSequence::new(timespec(120, 4, 4), NoteLength::Sixteenth);

        seq.set_step_trigger(0, DrumkitLabel::BassDrum, 1.0);

        seq.set_step_trigger(4, DrumkitLabel::BassDrum, 1.0);
        seq.set_step_trigger(4, DrumkitLabel::Snare, 1.0);

        seq.set_step_trigger(8, DrumkitLabel::BassDrum, 1.0);

        seq.set_step_trigger(12, DrumkitLabel::BassDrum, 1.0);
        seq.set_step_trigger(12, DrumkitLabel::Snare, 1.0);

        seq
    }

    fn steps_with_triggers<T>(range: Range<usize>, seq: &impl StepSequenceOps<T>) -> Vec<usize>
    where
        T: ConcreteSampleSetLabelling,
    {
        range
            .into_iter()
            .filter(|n| {
                matches!(seq.step(*n, 44100.try_into().unwrap()),
                         Some(info) if !info.triggers.is_empty())
            })
            .collect::<Vec<usize>>()
    }

    #[test]
    fn test_drumkit_seq_plain_step_lengths() {
        let seq = drumkitseq1();

        assert!((0..16).all(|n| f64_eq(
            seq.step(n, 44100.try_into().unwrap())
                .unwrap()
                .length_in_samples,
            5512.5
        )));
    }

    #[test]
    fn test_drumkit_seq_swing_step_lengths() {
        let mut seq = drumkitseq1();

        seq.set_timespec(TimeSpec {
            swing: 0.5.try_into().unwrap(),
            ..seq.timespec()
        });

        let rate: Samplerate = 44100.try_into().unwrap();

        assert!(f64_eq(
            seq.step(0, rate).unwrap().length_in_samples,
            8268.75
        ));
        assert!(f64_eq(
            seq.step(1, rate).unwrap().length_in_samples,
            2756.25
        ));
        assert!(f64_eq(
            seq.step(2, rate).unwrap().length_in_samples,
            8268.75
        ));
        assert!(f64_eq(
            seq.step(3, rate).unwrap().length_in_samples,
            2756.25
        ));
    }

    #[test]
    fn test_drumkit_seq_unset_trigger() {
        let mut seq = drumkitseq1();
        assert_eq!(steps_with_triggers(0..100, &seq), vec![0, 4, 8, 12]);

        seq.unset_step_trigger(12, DrumkitLabel::BassDrum);
        assert_eq!(steps_with_triggers(0..100, &seq), vec![0, 4, 8, 12]);

        seq.unset_step_trigger(12, DrumkitLabel::Snare);
        assert_eq!(steps_with_triggers(0..100, &seq), vec![0, 4, 8]);
    }

    #[test]
    fn test_drumkit_seq_set_trigger() {
        let mut seq = drumkitseq1();
        seq.set_step_trigger(1, DrumkitLabel::ClosedHihat, 1.0);
        assert_eq!(steps_with_triggers(0..100, &seq), vec![0, 1, 4, 8, 12]);

        assert!(seq
            .step(1, 44100.try_into().unwrap())
            .unwrap()
            .triggers
            .iter()
            .any(|trigger| trigger.label == DrumkitLabel::ClosedHihat))
    }
}
