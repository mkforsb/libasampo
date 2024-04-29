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
pub enum DrumkitLabel {
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
pub struct DrumkitLabelling {
    labels: HashMap<String, DrumkitLabel>,
}

impl Default for DrumkitLabelling {
    fn default() -> Self {
        Self::new()
    }
}

impl DrumkitLabelling {
    pub fn new() -> Self {
        DrumkitLabelling {
            labels: HashMap::new(),
        }
    }

    pub fn clear(&mut self) {
        self.labels.clear();
    }

    pub fn get(&self, sample: &Sample) -> Option<&DrumkitLabel> {
        self.labels.get(sample.uri())
    }

    pub fn set(&mut self, sample: &Sample, label: DrumkitLabel) {
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
    DrumkitLabelling(DrumkitLabelling),
}

impl SampleSetLabelling {
    pub fn has_label_for(&self, sample: &Sample) -> bool {
        match self {
            Self::DrumkitLabelling(labels) => labels.get(sample).is_some(),
        }
    }
    pub fn remove_label_for(&mut self, sample: &Sample) -> Result<(), Error> {
        match self {
            Self::DrumkitLabelling(labels) => labels.remove(sample),
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

impl SampleSetTrait for SampleSet {
    fn uuid(&self) -> &Uuid {
        match self {
            Self::BaseSampleSet(set) => set.uuid(),
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::BaseSampleSet(set) => set.name(),
        }
    }

    fn list(&self) -> Vec<&Sample> {
        match self {
            Self::BaseSampleSet(set) => set.list(),
        }
    }

    fn labelling(&self) -> Option<&SampleSetLabelling> {
        match self {
            Self::BaseSampleSet(set) => set.labelling(),
        }
    }

    fn labelling_mut(&mut self) -> Option<&mut SampleSetLabelling> {
        match self {
            Self::BaseSampleSet(set) => set.labelling_mut(),
        }
    }

    fn add(&mut self, source: &Source, sample: &Sample) -> Result<(), Error> {
        match self {
            Self::BaseSampleSet(set) => set.add(source, sample),
        }
    }

    fn remove(&mut self, sample: &Sample) -> Result<(), Error> {
        match self {
            Self::BaseSampleSet(set) => set.remove(sample),
        }
    }

    fn contains(&self, sample: &Sample) -> bool {
        match self {
            Self::BaseSampleSet(set) => set.contains(sample),
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            Self::BaseSampleSet(set) => set.is_empty(),
        }
    }

    fn cached_audio_hash_of(&self, sample: &Sample) -> Option<&str> {
        match self {
            Self::BaseSampleSet(set) => set.cached_audio_hash_of(sample),
        }
    }
}

#[cfg(test)]
mod tests {
    // TODO: why must `sample` be imported here, but not `fakesource`?
    use crate::testutils::{self, fakesource_from_json, sample, sample_from_json};

    use super::*;

    #[test]
    fn test_new_empty() {
        let mut samples = SampleSet::BaseSampleSet(BaseSampleSet::new("My Samples"));

        assert_eq!(samples.name(), "My Samples");
        assert!(samples.list().is_empty());
        assert!(samples.labelling().is_none());
        assert!(samples.remove(&testutils::sample!()).is_err());
        assert!(!samples.contains(&testutils::sample!()));
        assert!(samples.is_empty());
        assert!(samples
            .cached_audio_hash_of(&testutils::sample!())
            .is_none());
    }

    #[test]
    fn test_add_contains_not_empty_and_hash() {
        testutils::audiohash_for_test::RESULT.set(Some(|_| Ok("abc123".to_string())));

        let mut set = SampleSet::BaseSampleSet(BaseSampleSet::new("My Samples"));
        let source = testutils::fakesource!(json = r#"{ "list": [{"uri": "1.wav"}] }"#);

        assert!(set.add(&source, &source.list().unwrap()[0]).is_ok());
        assert!(set.contains(&source.list().unwrap()[0]));
        assert!(!set.is_empty());

        assert_eq!(
            set.cached_audio_hash_of(&source.list().unwrap()[0]),
            Some("abc123")
        );
    }

    #[test]
    fn test_list_sorted_by_uri() {
        testutils::audiohash_for_test::RESULT.set(Some(|_| Ok("abc123".to_string())));

        let mut set = SampleSet::BaseSampleSet(BaseSampleSet::new("My Samples"));
        let source = testutils::fakesource!(
            json = r#"{ "list": [{"uri": "3.wav"}, {"uri": "1.wav"}, {"uri": "2.wav"}] }"#
        );

        set.add(&source, &source.list().unwrap()[0]).unwrap();
        set.add(&source, &source.list().unwrap()[1]).unwrap();
        set.add(&source, &source.list().unwrap()[2]).unwrap();

        assert_eq!(
            set.list()
                .iter()
                .map(|sample| sample.uri())
                .collect::<Vec<_>>(),
            vec!["1.wav", "2.wav", "3.wav"]
        );
    }

    #[test]
    fn test_add_labelling() {
        let mut set = SampleSet::BaseSampleSet(BaseSampleSet::new("My Samples"));

        match &mut set {
            SampleSet::BaseSampleSet(bss) => bss.set_labelling(Some(
                SampleSetLabelling::DrumkitLabelling(DrumkitLabelling::new()),
            )),
        }

        assert!(match set.labelling() {
            Some(SampleSetLabelling::DrumkitLabelling(_)) => true,
            None => false,
        })
    }

    #[test]
    fn test_add_label() {
        testutils::audiohash_for_test::RESULT.set(Some(|_| Ok("abc123".to_string())));

        let mut set = SampleSet::BaseSampleSet(BaseSampleSet::new("My Samples"));
        let source = testutils::fakesource!(json = r#"{ "list": [{"uri": "1.wav"}] }"#);

        set.add(&source, &source.list().unwrap()[0]).unwrap();

        match &mut set {
            SampleSet::BaseSampleSet(bss) => bss.set_labelling(Some(
                SampleSetLabelling::DrumkitLabelling(DrumkitLabelling::new()),
            )),
        }

        assert!(!set
            .labelling()
            .unwrap()
            .has_label_for(&source.list().unwrap()[0]));

        if let Some(SampleSetLabelling::DrumkitLabelling(labels)) = set.labelling_mut() {
            labels.set(&source.list().unwrap()[0], DrumkitLabel::Clap);
        }

        assert!(set
            .labelling()
            .unwrap()
            .has_label_for(&source.list().unwrap()[0]));

        assert_eq!(
            match set.labelling() {
                Some(SampleSetLabelling::DrumkitLabelling(labels)) =>
                    labels.get(&source.list().unwrap()[0]),
                None => None,
            },
            Some(DrumkitLabel::Clap).as_ref()
        );
    }

    #[test]
    fn test_remove() {
        testutils::audiohash_for_test::RESULT.set(Some(|_| Ok("abc123".to_string())));

        let mut set = SampleSet::BaseSampleSet(BaseSampleSet::new("My Samples"));
        let source = testutils::fakesource!(json = r#"{ "list": [{"uri": "1.wav"}] }"#);

        set.add(&source, &source.list().unwrap()[0]).unwrap();

        assert!(set.remove(&source.list().unwrap()[0]).is_ok());
        assert!(set.is_empty());
    }

    #[test]
    fn test_remove_leads_to_hash_and_label_removed() {
        testutils::audiohash_for_test::RESULT.set(Some(|_| Ok("abc123".to_string())));

        let mut set = SampleSet::BaseSampleSet(BaseSampleSet::new("My Samples"));
        let source = testutils::fakesource!(json = r#"{ "list": [{"uri": "1.wav"}] }"#);

        set.add(&source, &source.list().unwrap()[0]).unwrap();

        match &mut set {
            SampleSet::BaseSampleSet(bss) => bss.set_labelling(Some(
                SampleSetLabelling::DrumkitLabelling(DrumkitLabelling::new()),
            )),
        }

        if let Some(SampleSetLabelling::DrumkitLabelling(labels)) = set.labelling_mut() {
            labels.set(&source.list().unwrap()[0], DrumkitLabel::Clap);
        }

        set.remove(&source.list().unwrap()[0]).unwrap();

        assert!(set
            .cached_audio_hash_of(&source.list().unwrap()[0])
            .is_none());

        assert!(!set
            .labelling()
            .unwrap()
            .has_label_for(&source.list().unwrap()[0]));
    }
}
