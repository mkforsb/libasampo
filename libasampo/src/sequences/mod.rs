// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use std::cmp::Ordering;

use crate::samplesets::DrumkitLabel;

mod render;
mod time;

pub use render::{
    DrumkitSampleLoader, DrumkitSequenceEvent, DrumkitSequenceRenderer, SampleSetSampleLoader,
};
pub use time::{NoteLength, Samplerate, Swing, TimeSignature, TimeSpec, BPM};
use uuid::Uuid;

#[cfg(feature = "audiothread-integration")]
pub mod drumkit_render_thread;

#[derive(Debug, Clone)]
pub struct Trigger {
    label: DrumkitLabel,
    amplitude: f32,
}

impl Trigger {
    pub fn label(&self) -> DrumkitLabel {
        self.label
    }

    pub fn amplitude(&self) -> f32 {
        self.amplitude
    }
}

impl PartialEq for Trigger {
    fn eq(&self, other: &Trigger) -> bool {
        !(self.label != other.label || self.amplitude != other.amplitude)
    }
}

impl Eq for Trigger {}

#[derive(Debug, Clone)]
pub struct StepInfo<'a> {
    length_in_samples_48k: f64,
    triggers: &'a Vec<Trigger>,
}

impl<'a> StepInfo<'a> {
    pub fn length_in_samples(&self, samplerate: Samplerate) -> f64 {
        self.length_in_samples_48k * ((samplerate.get() as f64) / 48000.0)
    }

    pub fn triggers(&self) -> &'a Vec<Trigger> {
        self.triggers
    }
}

pub trait StepSequenceOps {
    fn name(&self) -> &String;
    fn set_name(&mut self, name: impl Into<String>);
    fn uuid(&self) -> Uuid;
    fn timespec(&self) -> TimeSpec;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool;
    fn step_base_len(&self) -> NoteLength;
    fn step(&self, n: usize) -> Option<StepInfo>;
    fn set_timespec(&mut self, spec: TimeSpec);
    fn set_len(&mut self, len: usize);
    fn set_step_base_len(&mut self, len: NoteLength);
    fn clear(&mut self);
    fn clear_step(&mut self, n: usize);
    fn set_step_trigger(&mut self, n: usize, label: DrumkitLabel, amp: f32);
    fn unset_step_trigger(&mut self, n: usize, label: DrumkitLabel);

    #[cfg(any(test, feature = "testables"))]
    fn set_uuid(&mut self, uuid: Uuid);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrumkitSequence {
    uuid: Uuid,
    name: String,
    timespec: TimeSpec,
    step_base_length: NoteLength,
    steps: Vec<Vec<Trigger>>,
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
            uuid: Uuid::new_v4(),
            name: "Unnamed sequence".to_string(),
            timespec,
            step_base_length,
            steps,
        }
    }

    pub fn new_named(
        name: impl Into<String>,
        timespec: TimeSpec,
        step_base_length: NoteLength,
    ) -> Self {
        let mut result = Self::new(timespec, step_base_length);
        result.name = name.into();
        result
    }

    pub fn new_from(sequence: &DrumkitSequence) -> DrumkitSequence {
        DrumkitSequence {
            uuid: Uuid::new_v4(),
            name: sequence.name.clone(),
            timespec: sequence.timespec,
            step_base_length: sequence.step_base_length,
            steps: sequence.steps.clone(),
        }
    }

    #[cfg(not(any(test, feature = "testables")))]
    pub(crate) fn set_uuid(&mut self, uuid: Uuid) {
        self.uuid = uuid;
    }

    #[cfg(any(test, feature = "testables"))]
    pub fn set_uuid(&mut self, uuid: Uuid) {
        self.uuid = uuid;
    }

    pub fn labels_at_step(&self, n: usize) -> Option<Vec<DrumkitLabel>> {
        Some(
            self.steps
                .get(n)?
                .iter()
                .map(|t| t.label)
                .collect::<Vec<_>>(),
        )
    }
}

impl Default for DrumkitSequence {
    fn default() -> Self {
        DrumkitSequence::new(TimeSpec::new(120, 4, 4).unwrap(), NoteLength::Sixteenth)
    }
}

impl StepSequenceOps for DrumkitSequence {
    fn name(&self) -> &String {
        &self.name
    }

    fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into()
    }

    fn uuid(&self) -> Uuid {
        self.uuid
    }

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

    fn step(&self, n: usize) -> Option<StepInfo> {
        if let Some(triggers) = self.steps.get(n) {
            let base_len_in_samples = self
                .timespec
                .samples_per_note(48000.try_into().unwrap(), self.step_base_length);

            let sign = if n % 2 == 0 { 1.0 } else { -1.0 };

            Some(StepInfo {
                length_in_samples_48k: base_len_in_samples
                    * (1.0 + (sign * self.timespec.swing.get())),
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

    fn set_step_trigger(&mut self, n: usize, label: DrumkitLabel, amp: f32) {
        if let Some(v) = self.steps.get_mut(n) {
            v.retain(|trigger| trigger.label != label);
            v.push(Trigger {
                label,
                amplitude: amp,
            });
        }
    }

    fn unset_step_trigger(&mut self, n: usize, label: DrumkitLabel) {
        if let Some(v) = self.steps.get_mut(n) {
            v.retain(|trigger| trigger.label != label);
        }
    }

    #[cfg(any(test, feature = "testables"))]
    fn set_uuid(&mut self, uuid: Uuid) {
        self.uuid = uuid;
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
        seq.set_step_trigger(4, DrumkitLabel::SnareDrum, 1.0);

        seq.set_step_trigger(8, DrumkitLabel::BassDrum, 1.0);

        seq.set_step_trigger(12, DrumkitLabel::BassDrum, 1.0);
        seq.set_step_trigger(12, DrumkitLabel::SnareDrum, 1.0);

        seq
    }

    fn steps_with_triggers(range: Range<usize>, seq: &impl StepSequenceOps) -> Vec<usize> {
        range
            .into_iter()
            .filter(|n| matches!(seq.step(*n), Some(info) if !info.triggers.is_empty()))
            .collect::<Vec<usize>>()
    }

    #[test]
    fn test_drumkit_seq_plain_step_lengths() {
        let seq = drumkitseq1();

        assert!((0..16).all(|n| f64_eq(
            seq.step(n)
                .unwrap()
                .length_in_samples(44100.try_into().unwrap()),
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
            seq.step(0).unwrap().length_in_samples(rate),
            8268.75
        ));
        assert!(f64_eq(
            seq.step(1).unwrap().length_in_samples(rate),
            2756.25
        ));
        assert!(f64_eq(
            seq.step(2).unwrap().length_in_samples(rate),
            8268.75
        ));
        assert!(f64_eq(
            seq.step(3).unwrap().length_in_samples(rate),
            2756.25
        ));
    }

    #[test]
    fn test_drumkit_seq_unset_trigger() {
        let mut seq = drumkitseq1();
        assert_eq!(steps_with_triggers(0..100, &seq), vec![0, 4, 8, 12]);

        seq.unset_step_trigger(12, DrumkitLabel::BassDrum);
        assert_eq!(steps_with_triggers(0..100, &seq), vec![0, 4, 8, 12]);

        seq.unset_step_trigger(12, DrumkitLabel::SnareDrum);
        assert_eq!(steps_with_triggers(0..100, &seq), vec![0, 4, 8]);
    }

    #[test]
    fn test_drumkit_seq_set_trigger() {
        let mut seq = drumkitseq1();
        seq.set_step_trigger(1, DrumkitLabel::ClosedHihat, 1.0);
        assert_eq!(steps_with_triggers(0..100, &seq), vec![0, 1, 4, 8, 12]);

        assert!(seq
            .step(1)
            .unwrap()
            .triggers
            .iter()
            .any(|trigger| trigger.label == DrumkitLabel::ClosedHihat))
    }
}
