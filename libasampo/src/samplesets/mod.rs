// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::{
    errors::Error,
    samples::{Sample, SampleOps, SampleURI},
    sources::{Source, SourceOps},
};

#[cfg(not(test))]
use crate::audiohash::audio_hash;

#[cfg(test)]
use crate::testutils::audiohash_for_test::audio_hash;

pub mod export;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum DrumkitLabel {
    RimShot,
    Clap,
    ClosedHihat,
    OpenHihat,
    CrashCymbal,
    RideCymbal,
    Shaker,
    BassDrum,
    SnareDrum,
    LowTom,
    MidTom,
    HighTom,
    Perc1,
    Perc2,
    Perc3,
    Perc4,
}

pub trait ConcreteSampleSetLabelling {
    type Label: std::fmt::Debug + Clone;

    fn get(&self, uri: &SampleURI) -> Option<&Self::Label>;
    fn set(&mut self, uri: SampleURI, label: Self::Label);
}

pub trait SampleSetLabellingOps {
    fn contains(&self, uri: &SampleURI) -> bool;
    fn remove(&mut self, uri: &SampleURI) -> Result<(), Error>;
    fn clear(&mut self);
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DrumkitLabelling {
    labels: HashMap<SampleURI, DrumkitLabel>,
}

impl DrumkitLabelling {
    pub fn new() -> Self {
        DrumkitLabelling {
            labels: HashMap::new(),
        }
    }
}

impl ConcreteSampleSetLabelling for DrumkitLabelling {
    type Label = DrumkitLabel;

    fn get(&self, uri: &SampleURI) -> Option<&DrumkitLabel> {
        self.labels.get(uri)
    }

    fn set(&mut self, uri: SampleURI, label: DrumkitLabel) {
        self.labels.insert(uri, label);
    }
}

impl SampleSetLabellingOps for DrumkitLabelling {
    fn contains(&self, uri: &SampleURI) -> bool {
        self.labels.contains_key(uri)
    }

    fn clear(&mut self) {
        self.labels.clear()
    }

    fn len(&self) -> usize {
        self.labels.len()
    }

    fn is_empty(&self) -> bool {
        self.labels.is_empty()
    }

    fn remove(&mut self, uri: &SampleURI) -> Result<(), Error> {
        self.labels
            .remove(uri)
            .ok_or(Error::SampleSetSampleNotPresentError {
                uri: uri.to_string(),
            })
            .map(|_| ())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SampleSetLabelling {
    DrumkitLabelling(DrumkitLabelling),
}

impl SampleSetLabellingOps for SampleSetLabelling {
    fn contains(&self, uri: &SampleURI) -> bool {
        match self {
            Self::DrumkitLabelling(kit) => kit.contains(uri),
        }
    }

    fn remove(&mut self, uri: &SampleURI) -> Result<(), Error> {
        match self {
            Self::DrumkitLabelling(kit) => kit.remove(uri),
        }
    }

    fn clear(&mut self) {
        match self {
            Self::DrumkitLabelling(kit) => kit.clear(),
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::DrumkitLabelling(kit) => kit.len(),
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            Self::DrumkitLabelling(kit) => kit.is_empty(),
        }
    }
}

pub trait SampleSetOps {
    fn uuid(&self) -> &Uuid;
    fn name(&self) -> &str;
    fn list(&self) -> Vec<&Sample>;
    fn set_labelling(&mut self, labelling: Option<SampleSetLabelling>);
    fn labelling(&self) -> Option<&SampleSetLabelling>;
    fn labelling_mut(&mut self) -> Option<&mut SampleSetLabelling>;
    fn add(&mut self, source: &Source, sample: Sample) -> Result<(), Error>;
    fn add_with_hash(&mut self, sample: Sample, hash: String);
    fn remove(&mut self, sample: &Sample) -> Result<(), Error>;
    fn contains(&self, sample: &Sample) -> bool;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool;
    fn cached_audio_hash_of(&self, sample: &Sample) -> Option<&str>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BaseSampleSet {
    uuid: Uuid,
    name: String,
    samples: HashSet<Sample>,
    labelling: Option<SampleSetLabelling>,
    audio_hash: HashMap<SampleURI, String>,
}

impl BaseSampleSet {
    pub fn new(name: String) -> Self {
        BaseSampleSet {
            uuid: Uuid::new_v4(),
            name,
            samples: HashSet::new(),
            labelling: None,
            audio_hash: HashMap::new(),
        }
    }

    pub(crate) fn set_uuid(&mut self, uuid: Uuid) {
        self.uuid = uuid;
    }
}

impl SampleSetOps for BaseSampleSet {
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

    fn set_labelling(&mut self, labelling: Option<SampleSetLabelling>) {
        self.labelling = labelling;
    }

    fn labelling(&self) -> Option<&SampleSetLabelling> {
        self.labelling.as_ref()
    }

    fn labelling_mut(&mut self) -> Option<&mut SampleSetLabelling> {
        self.labelling.as_mut()
    }

    fn add(&mut self, source: &Source, sample: Sample) -> Result<(), Error> {
        self.audio_hash
            .insert(sample.uri().clone(), audio_hash(source.stream(&sample)?)?);

        self.samples.insert(sample);

        Ok(())
    }

    fn add_with_hash(&mut self, sample: Sample, hash: String) {
        self.audio_hash.insert(sample.uri().clone(), hash);

        self.samples.insert(sample);
    }

    fn remove(&mut self, sample: &Sample) -> Result<(), Error> {
        if !self.samples.remove(sample) {
            assert!(!self.audio_hash.contains_key(sample.uri()));
            assert!(
                self.labelling.is_none()
                    || !self.labelling.as_mut().unwrap().contains(sample.uri())
            );

            Err(Error::SampleSetSampleNotPresentError {
                uri: sample.uri().to_string(),
            })
        } else {
            self.audio_hash
                .remove(sample.uri())
                .expect("Should exist a matching key in audio_hash");

            self.labelling.as_mut().map(|x| x.remove(sample.uri()));
            Ok(())
        }
    }

    fn contains(&self, sample: &Sample) -> bool {
        self.samples.contains(sample)
    }

    fn len(&self) -> usize {
        self.samples.len()
    }

    fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    fn cached_audio_hash_of(&self, sample: &Sample) -> Option<&str> {
        self.audio_hash.get(sample.uri()).map(|x| x.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SampleSet {
    BaseSampleSet(BaseSampleSet),
}

impl SampleSetOps for SampleSet {
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

    fn set_labelling(&mut self, labelling: Option<SampleSetLabelling>) {
        match self {
            SampleSet::BaseSampleSet(set) => set.set_labelling(labelling),
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

    fn add(&mut self, source: &Source, sample: Sample) -> Result<(), Error> {
        match self {
            Self::BaseSampleSet(set) => set.add(source, sample),
        }
    }

    fn add_with_hash(&mut self, sample: Sample, hash: String) {
        match self {
            Self::BaseSampleSet(set) => set.add_with_hash(sample, hash),
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

    fn len(&self) -> usize {
        match self {
            Self::BaseSampleSet(set) => set.len(),
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
    use crate::testutils::{self, fakesource_from_json, s, sample, sample_from_json};

    use super::*;

    #[test]
    fn test_new_empty() {
        let mut samples = SampleSet::BaseSampleSet(BaseSampleSet::new(s("My Samples")));

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
        testutils::audiohash_for_test::RESULT.set(Some(|_| Ok(s("abc123"))));

        let mut set = SampleSet::BaseSampleSet(BaseSampleSet::new(s("My Samples")));
        let source = testutils::fakesource!(json = r#"{ "list": [{"uri": "1.wav"}] }"#);

        assert!(set.add(&source, source.list().unwrap()[0].clone()).is_ok());
        assert!(set.contains(&source.list().unwrap()[0]));
        assert!(!set.is_empty());

        assert_eq!(
            set.cached_audio_hash_of(&source.list().unwrap()[0]),
            Some("abc123")
        );
    }

    #[test]
    fn test_list_sorted_by_uri() {
        testutils::audiohash_for_test::RESULT.set(Some(|_| Ok(s("abc123"))));

        let mut set = SampleSet::BaseSampleSet(BaseSampleSet::new(s("My Samples")));
        let source = testutils::fakesource!(
            json = r#"{ "list": [{"uri": "3.wav"}, {"uri": "1.wav"}, {"uri": "2.wav"}] }"#
        );

        set.add(&source, source.list().unwrap()[0].clone()).unwrap();
        set.add(&source, source.list().unwrap()[1].clone()).unwrap();
        set.add(&source, source.list().unwrap()[2].clone()).unwrap();

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
        let mut set = SampleSet::BaseSampleSet(BaseSampleSet::new(s("My Samples")));

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
        testutils::audiohash_for_test::RESULT.set(Some(|_| Ok(s("abc123"))));

        let mut set = SampleSet::BaseSampleSet(BaseSampleSet::new(s("My Samples")));
        let source = testutils::fakesource!(json = r#"{ "list": [{"uri": "1.wav"}] }"#);

        set.add(&source, source.list().unwrap()[0].clone()).unwrap();

        match &mut set {
            SampleSet::BaseSampleSet(bss) => bss.set_labelling(Some(
                SampleSetLabelling::DrumkitLabelling(DrumkitLabelling::new()),
            )),
        }

        assert!(!set
            .labelling()
            .unwrap()
            .contains(source.list().unwrap()[0].uri()));

        if let Some(SampleSetLabelling::DrumkitLabelling(labels)) = set.labelling_mut() {
            labels.set(source.list().unwrap()[0].uri().clone(), DrumkitLabel::Clap);
        }

        assert!(set
            .labelling()
            .unwrap()
            .contains(source.list().unwrap()[0].uri()));

        assert_eq!(
            match set.labelling() {
                Some(SampleSetLabelling::DrumkitLabelling(labels)) =>
                    labels.get(source.list().unwrap()[0].uri()),
                None => None,
            },
            Some(DrumkitLabel::Clap).as_ref()
        );
    }

    #[test]
    fn test_remove() {
        testutils::audiohash_for_test::RESULT.set(Some(|_| Ok(s("abc123"))));

        let mut set = SampleSet::BaseSampleSet(BaseSampleSet::new(s("My Samples")));
        let source = testutils::fakesource!(json = r#"{ "list": [{"uri": "1.wav"}] }"#);

        set.add(&source, source.list().unwrap()[0].clone()).unwrap();

        assert!(set.remove(&source.list().unwrap()[0]).is_ok());
        assert!(set.is_empty());
    }

    #[test]
    fn test_remove_leads_to_hash_and_label_removed() {
        testutils::audiohash_for_test::RESULT.set(Some(|_| Ok(s("abc123"))));

        let mut set = SampleSet::BaseSampleSet(BaseSampleSet::new(s("My Samples")));
        let source = testutils::fakesource!(json = r#"{ "list": [{"uri": "1.wav"}] }"#);

        set.add(&source, source.list().unwrap()[0].clone()).unwrap();

        match &mut set {
            SampleSet::BaseSampleSet(bss) => bss.set_labelling(Some(
                SampleSetLabelling::DrumkitLabelling(DrumkitLabelling::new()),
            )),
        }

        if let Some(SampleSetLabelling::DrumkitLabelling(labels)) = set.labelling_mut() {
            labels.set(source.list().unwrap()[0].uri().clone(), DrumkitLabel::Clap);
        }

        set.remove(&source.list().unwrap()[0]).unwrap();

        assert!(set
            .cached_audio_hash_of(&source.list().unwrap()[0])
            .is_none());

        assert!(!set
            .labelling()
            .unwrap()
            .contains(source.list().unwrap()[0].uri()));
    }
}
