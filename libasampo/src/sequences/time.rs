// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use std::num::{NonZeroU16, NonZeroU32, NonZeroU8};

use crate::errors::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteLength {
    Eighth,
    Sixteenth,
}

impl NoteLength {
    pub fn reciprocal(&self) -> f64 {
        match self {
            NoteLength::Eighth => 8.0,
            NoteLength::Sixteenth => 16.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Samplerate(NonZeroU32);

impl Samplerate {
    pub fn new(value: u32) -> Result<Self, Error> {
        value.try_into()
    }

    pub fn get(&self) -> u32 {
        self.0.get()
    }
}

impl TryFrom<u32> for Samplerate {
    type Error = Error;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Ok(Self(NonZeroU32::new(value).ok_or(
            Error::ValueOutOfRangeError("Sample rate must be nonzero".to_string()),
        )?))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BPM(NonZeroU16);

impl BPM {
    pub fn new(value: u16) -> Result<Self, Error> {
        value.try_into()
    }

    pub fn get(&self) -> u16 {
        self.0.get()
    }
}

impl TryFrom<u16> for BPM {
    type Error = Error;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Ok(Self(NonZeroU16::new(value).ok_or(
            Error::ValueOutOfRangeError("BPM must be nonzero".to_string()),
        )?))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeSignature {
    upper: NonZeroU8,
    lower: NonZeroU8,
}

impl TimeSignature {
    pub fn new(upper: u8, lower: u8) -> Result<Self, Error> {
        (upper, lower).try_into()
    }

    pub fn upper(&self) -> u8 {
        self.upper.get()
    }

    pub fn lower(&self) -> u8 {
        self.lower.get()
    }
}

impl TryFrom<(u8, u8)> for TimeSignature {
    type Error = Error;

    fn try_from((upper, lower): (u8, u8)) -> Result<Self, Self::Error> {
        Ok(Self {
            upper: NonZeroU8::new(upper).ok_or(Error::ValueOutOfRangeError(
                "Time signature components must be nonzero".to_string(),
            ))?,
            lower: NonZeroU8::new(lower).ok_or(Error::ValueOutOfRangeError(
                "Time signature components must be nonzero".to_string(),
            ))?,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Swing(f64);

impl PartialEq for Swing {
    fn eq(&self, other: &Self) -> bool {
        (self.0 - other.0).abs() < f64::EPSILON
    }
}

impl Eq for Swing {}

impl Swing {
    pub fn new(value: f64) -> Result<Self, Error> {
        value.try_into()
    }

    pub fn get(&self) -> f64 {
        self.0
    }
}

impl TryFrom<f64> for Swing {
    type Error = Error;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        if (0.0..=1.0).contains(&value) {
            Ok(Swing(value))
        } else {
            Err(Error::ValueOutOfRangeError(
                "Swing value must be in the range [0.0, 1.0]".to_string(),
            ))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeSpec {
    pub bpm: BPM,
    pub signature: TimeSignature,
    pub swing: Swing,
}

impl TimeSpec {
    pub fn new(bpm: u16, sig_upper: u8, sig_lower: u8) -> Result<TimeSpec, Error> {
        TimeSpec::new_with_swing(bpm, sig_upper, sig_lower, 0.0)
    }

    pub fn new_with_swing(
        bpm: u16,
        sig_upper: u8,
        sig_lower: u8,
        swing: f64,
    ) -> Result<TimeSpec, Error> {
        Ok(TimeSpec {
            bpm: BPM::new(bpm)?,
            signature: TimeSignature::new(sig_upper, sig_lower)?,
            swing: Swing::new(swing)?,
        })
    }

    pub fn beats_per_bar(&self) -> u8 {
        self.signature.upper.get()
    }

    pub fn seconds_per_bar(&self) -> f64 {
        self.beats_per_bar() as f64 * self.seconds_per_beat()
    }

    pub fn beats_per_second(&self) -> f64 {
        self.bpm.0.get() as f64 / 60.0
    }

    pub fn seconds_per_beat(&self) -> f64 {
        1.0 / self.beats_per_second()
    }

    pub fn samples_per_beat(&self, samplerate: Samplerate) -> f64 {
        samplerate.0.get() as f64 * self.seconds_per_beat()
    }

    pub fn notes_per_beat(&self, note: NoteLength) -> f64 {
        (1.0 / self.signature.lower.get() as f64) * note.reciprocal()
    }

    pub fn seconds_per_note(&self, note: NoteLength) -> f64 {
        self.seconds_per_beat() / self.notes_per_beat(note)
    }

    pub fn samples_per_note(&self, samplerate: Samplerate, note: NoteLength) -> f64 {
        self.seconds_per_note(note) * samplerate.0.get() as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f64_eq(a: f64, b: f64) -> bool {
        (a - b).abs() < 0.0001
    }

    fn timespec(bpm: u16, sig_upper: u8, sig_lower: u8) -> TimeSpec {
        TimeSpec::new(bpm, sig_upper, sig_lower).unwrap()
    }

    #[test]
    fn test_invalid_values() {
        assert!(<u32 as TryInto<Samplerate>>::try_into(0).is_err());
        assert!(<u16 as TryInto<BPM>>::try_into(0).is_err());
        assert!(<(u8, u8) as TryInto<TimeSignature>>::try_into((0, 4)).is_err());
        assert!(<(u8, u8) as TryInto<TimeSignature>>::try_into((4, 0)).is_err());
        assert!(<(u8, u8) as TryInto<TimeSignature>>::try_into((0, 0)).is_err());
    }

    #[test]
    fn test_timespec() {
        assert_eq!(timespec(120, 4, 4).beats_per_bar(), 4);
        assert!(f64_eq(timespec(120, 4, 4).seconds_per_bar(), 2.0));
        assert!(f64_eq(timespec(120, 4, 4).beats_per_second(), 2.0));
        assert!(f64_eq(timespec(120, 4, 4).seconds_per_beat(), 0.5));
        assert!(f64_eq(
            timespec(120, 4, 4).samples_per_beat(44100.try_into().unwrap()),
            22050.0
        ));
        assert!(f64_eq(
            timespec(120, 4, 4).notes_per_beat(NoteLength::Eighth),
            2.0
        ));
        assert!(f64_eq(
            timespec(120, 4, 4).notes_per_beat(NoteLength::Sixteenth),
            4.0
        ));
        assert!(f64_eq(
            timespec(120, 4, 4).seconds_per_note(NoteLength::Eighth),
            0.25
        ));
        assert!(f64_eq(
            timespec(120, 4, 4).seconds_per_note(NoteLength::Sixteenth),
            0.125
        ));

        assert_eq!(timespec(140, 3, 4).beats_per_bar(), 3);
        assert!(f64_eq(timespec(140, 3, 4).seconds_per_bar(), 1.2857));
        assert!(f64_eq(timespec(140, 3, 4).beats_per_second(), 2.3333));
        assert!(f64_eq(timespec(140, 3, 4).seconds_per_beat(), 0.4286));
        assert!(f64_eq(
            timespec(140, 3, 4).samples_per_beat(44100.try_into().unwrap()),
            18900.0
        ));
        assert!(f64_eq(
            timespec(140, 3, 4).notes_per_beat(NoteLength::Eighth),
            2.0
        ));
        assert!(f64_eq(
            timespec(140, 3, 4).notes_per_beat(NoteLength::Sixteenth),
            4.0
        ));
        assert!(f64_eq(
            timespec(140, 3, 4).seconds_per_note(NoteLength::Eighth),
            0.2143
        ));
        assert!(f64_eq(
            timespec(140, 3, 4).seconds_per_note(NoteLength::Sixteenth),
            0.1071
        ));
    }
}
