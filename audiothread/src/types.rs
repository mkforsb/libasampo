// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use std::num::{NonZeroU32, NonZeroU8, NonZeroUsize};

use crate::error::ValueOutOfRangeError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Samplerate(NonZeroU32);

impl Samplerate {
    pub fn new(value: u32) -> Result<Self, ValueOutOfRangeError> {
        value.try_into()
    }

    pub fn get(&self) -> u32 {
        self.0.get()
    }
}

impl TryFrom<u32> for Samplerate {
    type Error = ValueOutOfRangeError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Ok(Samplerate(NonZeroU32::new(value).ok_or(
            ValueOutOfRangeError("Sample rate must be greater than zero".to_string()),
        )?))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NumChannels(NonZeroU8);

impl NumChannels {
    pub fn new(value: u8) -> Result<Self, ValueOutOfRangeError> {
        value.try_into()
    }

    pub fn get(&self) -> u8 {
        self.0.get()
    }
}

impl TryFrom<u8> for NumChannels {
    type Error = ValueOutOfRangeError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Ok(NumChannels(NonZeroU8::new(value).ok_or(
            ValueOutOfRangeError("Channel count must be greater than zero".to_string()),
        )?))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NumFrames(usize);

impl NumFrames {
    pub fn new(value: usize) -> Self {
        NumFrames(value)
    }

    pub fn get(&self) -> usize {
        self.0
    }
}

impl From<usize> for NumFrames {
    fn from(value: usize) -> Self {
        NumFrames(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NonZeroNumFrames(NonZeroUsize);

impl NonZeroNumFrames {
    pub fn new(value: usize) -> Result<Self, ValueOutOfRangeError> {
        value.try_into()
    }

    pub fn get(&self) -> usize {
        self.0.get()
    }
}

impl TryFrom<usize> for NonZeroNumFrames {
    type Error = ValueOutOfRangeError;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        Ok(NonZeroNumFrames(NonZeroUsize::new(value).ok_or(
            ValueOutOfRangeError("Frame count must be greater than zero".to_string()),
        )?))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AudioSpec {
    pub samplerate: Samplerate,
    pub channels: NumChannels,
}

impl AudioSpec {
    pub fn new(samplerate: u32, channels: u8) -> Result<Self, ValueOutOfRangeError> {
        Ok(Self {
            samplerate: samplerate.try_into()?,
            channels: channels.try_into()?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamState {
    Streaming,
    Complete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    Lowest,
    Low,
    Medium,
    High,
}
