// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::{
    errors::Error,
    samples::{Sample, SampleTrait},
    sources::{Source, SourceTrait},
};

#[cfg(not(test))]
use crate::audiohash::audio_hash;

#[cfg(test)]
use crate::testutils::audiohash_for_test::audio_hash;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DrumPadLabel {
    BassDrum,
    Rimshot,
    Snare,
    Clap,
    ClosedHihat,
    OpenHihat,
    CrashCymbal,
    RideCymbal,
    Tom1,
    Tom2,
    Tom3,
}

#[derive(Clone, Debug)]
pub struct DrumPadLabelling {
    labels: HashMap<String, DrumPadLabel>,
}

impl Default for DrumPadLabelling {
    fn default() -> Self {
        Self::new()
    }
}

impl DrumPadLabelling {
    pub fn new() -> Self {
        DrumPadLabelling {
            labels: HashMap::new(),
        }
    }

    pub fn clear(&mut self) {
        self.labels.clear();
    }

    pub fn get(&self, sample: &Sample) -> Option<&DrumPadLabel> {
        self.labels.get(sample.uri())
    }

    pub fn set(&mut self, sample: &Sample, label: DrumPadLabel) {
        self.labels.insert(sample.uri().to_string(), label);
    }

    pub fn remove(&mut self, sample: &Sample) -> Result<(), Error> {
        self.labels
            .remove(sample.uri())
            .ok_or(Error::SampleSetSampleNotPresentError {
                uri: sample.uri().to_string(),
            })
            .map(|_| ())
    }
}

#[derive(Clone, Debug)]
pub enum SampleSetLabelling {
    DrumPadLabelling(DrumPadLabelling),
}

impl SampleSetLabelling {
    pub fn has_label_for(&self, sample: &Sample) -> bool {
        match self {
            Self::DrumPadLabelling(labels) => labels.get(sample).is_some(),
        }
    }
    pub fn remove_label_for(&mut self, sample: &Sample) -> Result<(), Error> {
        match self {
            Self::DrumPadLabelling(labels) => labels.remove(sample),
        }
    }
}

pub trait SampleSetTrait {
    fn uuid(&self) -> &Uuid;
    fn name(&self) -> &str;
    fn list(&self) -> Vec<&Sample>;
    fn labelling(&self) -> Option<&SampleSetLabelling>;
    fn labelling_mut(&mut self) -> Option<&mut SampleSetLabelling>;
    fn add(&mut self, source: &Source, sample: &Sample) -> Result<(), Error>;
    fn remove(&mut self, sample: &Sample) -> Result<(), Error>;
    fn contains(&self, sample: &Sample) -> bool;
    fn is_empty(&self) -> bool;
    fn cached_audio_hash_of(&self, sample: &Sample) -> Option<&str>;
}

#[derive(Clone, Debug)]
pub struct BaseSampleSet {
    uuid: Uuid,
    name: String,
    samples: HashSet<Sample>,
    labelling: Option<SampleSetLabelling>,
    audio_hash: HashMap<String, String>,
}

impl BaseSampleSet {
    pub fn new(name: &str) -> Self {
        BaseSampleSet {
            uuid: Uuid::new_v4(),
            name: name.to_string(),
            samples: HashSet::new(),
            labelling: None,
            audio_hash: HashMap::new(),
        }
    }

    pub fn set_labelling(&mut self, labelling: Option<SampleSetLabelling>) {
        self.labelling = labelling;
    }
}

impl SampleSetTrait for BaseSampleSet {
    fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn list(&self) -> Vec<&Sample> {
        let mut result = self.samples.iter().collect::<Vec<_>>();
        result.sort_by(|a: &&Sample, b: &&Sample| a.uri().cmp(b.uri()));

        result
    }

    fn labelling(&self) -> Option<&SampleSetLabelling> {
        self.labelling.as_ref()
    }

    fn labelling_mut(&mut self) -> Option<&mut SampleSetLabelling> {
        self.labelling.as_mut()
    }

    fn add(&mut self, source: &Source, sample: &Sample) -> Result<(), Error> {
        self.samples.insert(sample.clone());
        self.audio_hash.insert(
            sample.uri().to_string(),
            audio_hash(source.stream(sample)?)?,
        );

        Ok(())
    }

    fn remove(&mut self, sample: &Sample) -> Result<(), Error> {
        if !self.samples.remove(sample) {
            assert!(!self.audio_hash.contains_key(sample.uri()));
            assert!(
                self.labelling.is_none() || !self.labelling.as_mut().unwrap().has_label_for(sample)
            );

            Err(Error::SampleSetSampleNotPresentError {
                uri: sample.uri().to_string(),
            })
        } else {
            self.audio_hash
                .remove(sample.uri())
                .expect("Should exist a matching key in audio_hash");

            self.labelling.as_mut().map(|x| x.remove_label_for(sample));
            Ok(())
        }
    }

    fn contains(&self, sample: &Sample) -> bool {
        self.samples.contains(sample)
    }

    fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    fn cached_audio_hash_of(&self, sample: &Sample) -> Option<&str> {
        self.audio_hash.get(sample.uri()).map(|x| x.as_str())
    }
}

#[derive(Clone, Debug)]
pub enum SampleSet {
    BaseSampleSet(BaseSampleSet),
}
