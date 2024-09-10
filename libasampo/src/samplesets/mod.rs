// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use std::collections::HashMap;

use uuid::Uuid;

use crate::{
    errors::Error,
    samples::{Sample, SampleOps},
    sources::{Source, SourceOps},
};

pub mod export;

#[cfg(not(test))]
use crate::audiohash::audio_hash;

#[cfg(test)]
use crate::testutils::audiohash_for_test::audio_hash;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

#[derive(Debug, Clone, Copy)]
pub enum Label {
    DrumkitLabel(DrumkitLabel),
}

impl From<DrumkitLabel> for Label {
    fn from(value: DrumkitLabel) -> Self {
        Label::DrumkitLabel(value)
    }
}

impl TryFrom<Label> for DrumkitLabel {
    type Error = Error;

    fn try_from(value: Label) -> Result<Self, Self::Error> {
        match value {
            Label::DrumkitLabel(label) => Ok(label),
        }
    }
}

impl<T> PartialEq<T> for Label
where
    T: Into<Label> + Copy,
{
    fn eq(&self, other: &T) -> bool {
        match (self, <T as Into<Label>>::into(*other)) {
            (Label::DrumkitLabel(a), Label::DrumkitLabel(b)) => *a == b,
        }
    }
}

impl Eq for Label {}

pub trait SampleSetOps {
    fn uuid(&self) -> Uuid;
    fn name(&self) -> &str;
    fn list(&self) -> Vec<&Sample>;
    fn add(&mut self, source: &Source, sample: Sample) -> Result<(), Error>;
    fn add_with_hash(&mut self, sample: Sample, hash: String);
    fn remove(&mut self, sample: &Sample) -> Result<(), Error>;
    fn contains(&self, sample: &Sample) -> bool;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool;
    fn cached_audio_hash_of(&self, sample: &Sample) -> Result<&str, Error>;

    fn set_label<T, U>(&mut self, sample: &Sample, label: U) -> Result<(), Error>
    where
        T: Into<Label>,
        U: Into<Option<T>>;

    fn get_label<T>(&self, sample: &Sample) -> Result<Option<T>, Error>
    where
        T: TryFrom<Label>;

    // TODO: what is the point of the `bool` value?
    fn clear_label(&mut self, sample: &Sample) -> Result<bool, Error>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    label: Option<Label>,
    audio_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseSampleSet {
    uuid: Uuid,
    name: String,
    samples: HashMap<Sample, Entry>,
}

impl BaseSampleSet {
    pub fn new(name: impl Into<String>) -> BaseSampleSet {
        BaseSampleSet {
            uuid: Uuid::new_v4(),
            name: name.into(),
            samples: HashMap::new(),
        }
    }

    pub(crate) fn set_uuid(&mut self, uuid: Uuid) {
        self.uuid = uuid;
    }
}

impl SampleSetOps for BaseSampleSet {
    fn uuid(&self) -> Uuid {
        self.uuid
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn list(&self) -> Vec<&Sample> {
        let mut result = self.samples.keys().collect::<Vec<_>>();
        result.sort_by(|a, b| a.uri().cmp(b.uri()));

        result
    }

    fn add(&mut self, source: &Source, sample: Sample) -> Result<(), Error> {
        let audio_hash = audio_hash(source.stream(&sample)?)?;
        self.samples.insert(
            sample,
            Entry {
                label: None,
                audio_hash,
            },
        );
        Ok(())
    }

    fn add_with_hash(&mut self, sample: Sample, hash: String) {
        self.samples.insert(
            sample,
            Entry {
                label: None,
                audio_hash: hash,
            },
        );
    }

    fn remove(&mut self, sample: &Sample) -> Result<(), Error> {
        self.samples
            .remove(sample)
            .ok_or(Error::SampleSetSampleNotPresentError {
                uri: sample.uri().to_string(),
            })?;

        Ok(())
    }

    fn contains(&self, sample: &Sample) -> bool {
        self.samples.contains_key(sample)
    }

    fn len(&self) -> usize {
        self.samples.len()
    }

    fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    fn cached_audio_hash_of(&self, sample: &Sample) -> Result<&str, Error> {
        self.samples
            .get(sample)
            .map(|s| s.audio_hash.as_str())
            .ok_or(Error::SampleSetSampleNotPresentError {
                uri: sample.uri().to_string(),
            })
    }

    fn set_label<T, U>(&mut self, sample: &Sample, label: U) -> Result<(), Error>
    where
        T: Into<Label>,
        U: Into<Option<T>>,
    {
        if self.samples.contains_key(sample) {
            if let Some(label) = label.into() {
                self.samples.get_mut(sample).unwrap().label = Some(label.into());
            } else {
                self.samples.get_mut(sample).unwrap().label = None;
            }
            Ok(())
        } else {
            Err(Error::SampleSetSampleNotPresentError {
                uri: sample.uri().to_string(),
            })
        }
    }

    fn get_label<T>(&self, sample: &Sample) -> Result<Option<T>, Error>
    where
        T: TryFrom<Label>,
    {
        Ok(self
            .samples
            .get(sample)
            .ok_or(Error::SampleSetSampleNotPresentError {
                uri: sample.uri().to_string(),
            })?
            .label
            .and_then(|x| x.try_into().ok()))
    }

    fn clear_label(&mut self, sample: &Sample) -> Result<bool, Error> {
        Ok(self
            .samples
            .get_mut(sample)
            .ok_or(Error::SampleSetSampleNotPresentError {
                uri: sample.uri().to_string(),
            })
            .is_ok_and(|x| {
                x.label = None;
                true
            }))
    }
}

#[cfg(any(test, feature = "fakes"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeSampleSet {
    pub uuid: Uuid,
    pub name: String,
    pub samples: HashMap<Sample, FakeEntry>,
}

#[cfg(any(test, feature = "fakes"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeEntry {
    pub label: Option<Label>,
    pub audio_hash: String,
}

#[cfg(any(test, feature = "fakes"))]
impl SampleSetOps for FakeSampleSet {
    fn uuid(&self) -> Uuid {
        self.uuid
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn list(&self) -> Vec<&Sample> {
        self.samples.keys().collect()
    }

    fn add(&mut self, _source: &Source, sample: Sample) -> Result<(), Error> {
        self.samples.insert(
            sample,
            FakeEntry {
                label: None,
                audio_hash: String::from(""),
            },
        );

        Ok(())
    }

    fn add_with_hash(&mut self, sample: Sample, hash: String) {
        self.samples.insert(
            sample,
            FakeEntry {
                label: None,
                audio_hash: hash,
            },
        );
    }

    fn remove(&mut self, sample: &Sample) -> Result<(), Error> {
        let uri = sample.uri().to_string();
        self.samples
            .remove(sample)
            .ok_or(Error::SampleSetSampleNotPresentError { uri })?;
        Ok(())
    }

    fn contains(&self, sample: &Sample) -> bool {
        self.samples.contains_key(sample)
    }

    fn len(&self) -> usize {
        self.samples.len()
    }

    fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    fn cached_audio_hash_of(&self, sample: &Sample) -> Result<&str, Error> {
        let uri = sample.uri().to_string();
        Ok(self
            .samples
            .get(sample)
            .ok_or(Error::SampleSetSampleNotPresentError { uri })?
            .audio_hash
            .as_str())
    }

    fn set_label<T, U>(&mut self, sample: &Sample, label: U) -> Result<(), Error>
    where
        T: Into<Label>,
        U: Into<Option<T>>,
    {
        let uri = sample.uri().to_string();
        self.samples
            .get_mut(sample)
            .ok_or(Error::SampleSetSampleNotPresentError { uri })?
            .label = label.into().map(|label| label.into());
        Ok(())
    }

    fn get_label<T>(&self, sample: &Sample) -> Result<Option<T>, Error>
    where
        T: TryFrom<Label>,
    {
        let uri = sample.uri().to_string();
        match self
            .samples
            .get(sample)
            .ok_or(Error::SampleSetSampleNotPresentError { uri })?
            .label
        {
            Some(label) => Ok(label.try_into().ok()),
            None => Ok(None),
        }
    }

    fn clear_label(&mut self, sample: &Sample) -> Result<bool, Error> {
        let uri = sample.uri().to_string();
        self.samples
            .get_mut(sample)
            .ok_or(Error::SampleSetSampleNotPresentError { uri })?
            .label = None;

        Ok(true)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SampleSet {
    BaseSampleSet(BaseSampleSet),

    #[cfg(any(test, feature = "fakes"))]
    FakeSampleSet(FakeSampleSet),
}

impl SampleSetOps for SampleSet {
    fn uuid(&self) -> Uuid {
        match self {
            Self::BaseSampleSet(set) => set.uuid(),

            #[cfg(any(test, feature = "fakes"))]
            Self::FakeSampleSet(set) => set.uuid(),
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::BaseSampleSet(set) => set.name(),

            #[cfg(any(test, feature = "fakes"))]
            Self::FakeSampleSet(set) => set.name(),
        }
    }

    fn list(&self) -> Vec<&Sample> {
        match self {
            Self::BaseSampleSet(set) => set.list(),

            #[cfg(any(test, feature = "fakes"))]
            Self::FakeSampleSet(set) => set.list(),
        }
    }

    fn add(&mut self, source: &Source, sample: Sample) -> Result<(), Error> {
        match self {
            Self::BaseSampleSet(set) => set.add(source, sample),

            #[cfg(any(test, feature = "fakes"))]
            Self::FakeSampleSet(set) => set.add(source, sample),
        }
    }

    fn add_with_hash(&mut self, sample: Sample, hash: String) {
        match self {
            Self::BaseSampleSet(set) => set.add_with_hash(sample, hash),

            #[cfg(any(test, feature = "fakes"))]
            Self::FakeSampleSet(set) => set.add_with_hash(sample, hash),
        }
    }

    fn remove(&mut self, sample: &Sample) -> Result<(), Error> {
        match self {
            Self::BaseSampleSet(set) => set.remove(sample),

            #[cfg(any(test, feature = "fakes"))]
            Self::FakeSampleSet(set) => set.remove(sample),
        }
    }

    fn contains(&self, sample: &Sample) -> bool {
        match self {
            Self::BaseSampleSet(set) => set.contains(sample),

            #[cfg(any(test, feature = "fakes"))]
            Self::FakeSampleSet(set) => set.contains(sample),
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::BaseSampleSet(set) => set.len(),

            #[cfg(any(test, feature = "fakes"))]
            Self::FakeSampleSet(set) => set.len(),
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            Self::BaseSampleSet(set) => set.is_empty(),

            #[cfg(any(test, feature = "fakes"))]
            Self::FakeSampleSet(set) => set.is_empty(),
        }
    }

    fn cached_audio_hash_of(&self, sample: &Sample) -> Result<&str, Error> {
        match self {
            Self::BaseSampleSet(set) => set.cached_audio_hash_of(sample),

            #[cfg(any(test, feature = "fakes"))]
            Self::FakeSampleSet(set) => set.cached_audio_hash_of(sample),
        }
    }

    fn set_label<T, U>(&mut self, sample: &Sample, label: U) -> Result<(), Error>
    where
        T: Into<Label>,
        U: Into<Option<T>>,
    {
        match self {
            Self::BaseSampleSet(set) => set.set_label(sample, label),

            #[cfg(any(test, feature = "fakes"))]
            Self::FakeSampleSet(set) => set.set_label(sample, label),
        }
    }

    fn get_label<T>(&self, sample: &Sample) -> Result<Option<T>, Error>
    where
        T: TryFrom<Label>,
    {
        match self {
            Self::BaseSampleSet(set) => set.get_label(sample),

            #[cfg(any(test, feature = "fakes"))]
            Self::FakeSampleSet(set) => set.get_label(sample),
        }
    }

    fn clear_label(&mut self, sample: &Sample) -> Result<bool, Error> {
        match self {
            Self::BaseSampleSet(set) => set.clear_label(sample),

            #[cfg(any(test, feature = "fakes"))]
            Self::FakeSampleSet(set) => set.clear_label(sample),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testutils::{self, s, sample};

    use super::*;

    #[test]
    fn test_new_empty() {
        let mut samples = SampleSet::BaseSampleSet(BaseSampleSet::new("My Samples"));

        assert_eq!(samples.name(), "My Samples");
        assert!(samples.list().is_empty());
        assert!(samples.remove(&testutils::sample!()).is_err());
        assert!(!samples.contains(&testutils::sample!()));
        assert!(samples
            .set_label(&testutils::sample!(), DrumkitLabel::BassDrum)
            .is_err());
        assert!(samples
            .get_label::<DrumkitLabel>(&testutils::sample!())
            .is_err());
        assert!(samples.is_empty());
        assert!(samples.cached_audio_hash_of(&testutils::sample!()).is_err());
    }

    #[test]
    fn test_add_contains_not_empty_and_hash() {
        testutils::audiohash_for_test::RESULT.set(Some(|_| Ok(s("abc123"))));

        let mut set = SampleSet::BaseSampleSet(BaseSampleSet::new("My Samples"));
        let source = testutils::fakesource!(json = r#"{ "list": [{"uri": "1.wav"}] }"#);

        assert!(set.add(&source, source.list().unwrap()[0].clone()).is_ok());
        assert!(set.contains(&source.list().unwrap()[0]));
        assert!(!set.is_empty());

        assert_eq!(
            set.cached_audio_hash_of(&source.list().unwrap()[0])
                .unwrap(),
            "abc123"
        );
    }

    #[test]
    fn test_list_sorted_by_uri() {
        testutils::audiohash_for_test::RESULT.set(Some(|_| Ok(s("abc123"))));

        let mut set = SampleSet::BaseSampleSet(BaseSampleSet::new("My Samples"));
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
    fn test_add_label() {
        testutils::audiohash_for_test::RESULT.set(Some(|_| Ok(s("abc123"))));

        let mut set = SampleSet::BaseSampleSet(BaseSampleSet::new("My Samples"));
        let source = testutils::fakesource!(json = r#"{ "list": [{"uri": "1.wav"}] }"#);
        let sample = &source.list().unwrap()[0];

        set.add(&source, sample.clone()).unwrap();

        assert!(set.set_label(sample, DrumkitLabel::Clap).is_ok());
        assert!(set.get_label::<DrumkitLabel>(sample).is_ok());
        assert!(set.get_label::<DrumkitLabel>(sample).unwrap() == Some(DrumkitLabel::Clap));

        assert!(matches!(
            set.get_label::<Label>(sample).unwrap(),
            Some(Label::DrumkitLabel(DrumkitLabel::Clap))
        ));

        assert!(set.set_label(sample, DrumkitLabel::MidTom).is_ok());
        assert!(set.get_label::<DrumkitLabel>(sample).unwrap() == Some(DrumkitLabel::MidTom));

        assert!(set.set_label::<Label, Option<Label>>(sample, None).is_ok());
        assert!(set.get_label::<DrumkitLabel>(sample).unwrap().is_none());
    }

    #[test]
    fn test_remove() {
        testutils::audiohash_for_test::RESULT.set(Some(|_| Ok(s("abc123"))));

        let mut set = SampleSet::BaseSampleSet(BaseSampleSet::new("My Samples"));
        let source = testutils::fakesource!(json = r#"{ "list": [{"uri": "1.wav"}] }"#);

        set.add(&source, source.list().unwrap()[0].clone()).unwrap();

        assert!(set.remove(&source.list().unwrap()[0]).is_ok());
        assert!(set.is_empty());
    }

    #[test]
    fn test_remove_leads_to_hash_and_label_removed() {
        testutils::audiohash_for_test::RESULT.set(Some(|_| Ok(s("abc123"))));

        let mut set = SampleSet::BaseSampleSet(BaseSampleSet::new("My Samples"));
        let source = testutils::fakesource!(json = r#"{ "list": [{"uri": "1.wav"}] }"#);
        let sample = &source.list().unwrap()[0];

        set.add(&source, sample.clone()).unwrap();
        set.set_label(sample, DrumkitLabel::Clap).unwrap();
        set.remove(&source.list().unwrap()[0]).unwrap();

        assert!(set.cached_audio_hash_of(sample).is_err());
        assert!(set.get_label::<Label>(sample).is_err());
    }
}
