// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use std::collections::HashMap;

use uuid::Uuid;

use crate::{
    audiohash::{AudioHasher, Md5AudioHasher},
    errors::Error,
    samples::{Sample, SampleOps},
    sources::{Source, SourceOps},
};

pub mod export;

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

    fn set_label<T>(&mut self, sample: &Sample, label: Option<T>) -> Result<(), Error>
    where
        T: Into<Label>;

    fn get_label<T>(&self, sample: &Sample) -> Result<Option<T>, Error>
    where
        T: TryFrom<Label>;

    #[cfg(any(test, feature = "testables"))]
    fn set_uuid(&mut self, uuid: Uuid);
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    label: Option<Label>,
    audio_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseSampleSet<H: AudioHasher = Md5AudioHasher> {
    uuid: Uuid,
    name: String,
    samples: HashMap<Sample, Entry>,
    _phantom: std::marker::PhantomData<H>,
}

impl BaseSampleSet {
    pub fn new(name: impl Into<String>) -> BaseSampleSet {
        Self::new_with_hasher::<Md5AudioHasher>(name)
    }

    pub fn new_with_hasher<H>(name: impl Into<String>) -> BaseSampleSet<H>
    where
        H: AudioHasher,
    {
        BaseSampleSet {
            uuid: Uuid::new_v4(),
            name: name.into(),
            samples: HashMap::new(),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Used in deserialization
    pub(crate) fn set_uuid(&mut self, uuid: Uuid) {
        self.uuid = uuid;
    }
}

impl<H> SampleSetOps for BaseSampleSet<H>
where
    H: AudioHasher,
{
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
        let audio_hash = H::audio_hash(source.stream(&sample)?)?;
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

    fn set_label<T>(&mut self, sample: &Sample, label: Option<T>) -> Result<(), Error>
    where
        T: Into<Label>,
    {
        if self.samples.contains_key(sample) {
            self.samples.get_mut(sample).unwrap().label = label.map(Into::into);
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

    #[cfg(any(test, feature = "testables"))]
    fn set_uuid(&mut self, uuid: Uuid) {
        self.uuid = uuid;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SampleSet<H: AudioHasher = Md5AudioHasher> {
    BaseSampleSet(BaseSampleSet<H>),
}

impl<H> SampleSetOps for SampleSet<H>
where
    H: AudioHasher,
{
    fn uuid(&self) -> Uuid {
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

    fn cached_audio_hash_of(&self, sample: &Sample) -> Result<&str, Error> {
        match self {
            Self::BaseSampleSet(set) => set.cached_audio_hash_of(sample),
        }
    }

    fn set_label<T>(&mut self, sample: &Sample, label: Option<T>) -> Result<(), Error>
    where
        T: Into<Label>,
    {
        match self {
            Self::BaseSampleSet(set) => set.set_label(sample, label),
        }
    }

    fn get_label<T>(&self, sample: &Sample) -> Result<Option<T>, Error>
    where
        T: TryFrom<Label>,
    {
        match self {
            Self::BaseSampleSet(set) => set.get_label(sample),
        }
    }

    #[cfg(any(test, feature = "testables"))]
    fn set_uuid(&mut self, uuid: Uuid) {
        match self {
            Self::BaseSampleSet(set) => set.set_uuid(uuid),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testutils::{self, sample};

    use super::*;

    struct DummyHasher;

    impl AudioHasher for DummyHasher {
        fn audio_hash(_reader: crate::sources::SourceReader) -> Result<String, Error> {
            Ok("abc123".to_string())
        }
    }

    #[test]
    fn test_new_empty() {
        let mut samples = SampleSet::BaseSampleSet(BaseSampleSet::new("My Samples"));

        assert_eq!(samples.name(), "My Samples");
        assert!(samples.list().is_empty());
        assert!(samples.remove(&testutils::sample!()).is_err());
        assert!(!samples.contains(&testutils::sample!()));
        assert!(samples
            .set_label(&testutils::sample!(), Some(DrumkitLabel::BassDrum))
            .is_err());
        assert!(samples
            .get_label::<DrumkitLabel>(&testutils::sample!())
            .is_err());
        assert!(samples.is_empty());
        assert!(samples.cached_audio_hash_of(&testutils::sample!()).is_err());
    }

    #[test]
    fn test_add_contains_not_empty_and_hash() {
        let mut set =
            SampleSet::BaseSampleSet(BaseSampleSet::new_with_hasher::<DummyHasher>("My Samples"));

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
        let mut set =
            SampleSet::BaseSampleSet(BaseSampleSet::new_with_hasher::<DummyHasher>("My Samples"));

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
        let mut set =
            SampleSet::BaseSampleSet(BaseSampleSet::new_with_hasher::<DummyHasher>("My Samples"));

        let source = testutils::fakesource!(json = r#"{ "list": [{"uri": "1.wav"}] }"#);
        let sample = &source.list().unwrap()[0];

        set.add(&source, sample.clone()).unwrap();

        assert!(set.set_label(sample, Some(DrumkitLabel::Clap)).is_ok());
        assert!(set.get_label::<DrumkitLabel>(sample).is_ok());
        assert!(set.get_label::<DrumkitLabel>(sample).unwrap() == Some(DrumkitLabel::Clap));

        assert!(matches!(
            set.get_label::<Label>(sample).unwrap(),
            Some(Label::DrumkitLabel(DrumkitLabel::Clap))
        ));

        assert!(set.set_label(sample, Some(DrumkitLabel::MidTom)).is_ok());
        assert!(set.get_label::<DrumkitLabel>(sample).unwrap() == Some(DrumkitLabel::MidTom));

        assert!(set.set_label(sample, None::<Label>).is_ok());
        assert!(set.get_label::<DrumkitLabel>(sample).unwrap().is_none());
    }

    #[test]
    fn test_remove() {
        let mut set =
            SampleSet::BaseSampleSet(BaseSampleSet::new_with_hasher::<DummyHasher>("My Samples"));

        let source = testutils::fakesource!(json = r#"{ "list": [{"uri": "1.wav"}] }"#);

        set.add(&source, source.list().unwrap()[0].clone()).unwrap();

        assert!(set.remove(&source.list().unwrap()[0]).is_ok());
        assert!(set.is_empty());
    }

    #[test]
    fn test_remove_leads_to_hash_and_label_removed() {
        let mut set =
            SampleSet::BaseSampleSet(BaseSampleSet::new_with_hasher::<DummyHasher>("My Samples"));

        let source = testutils::fakesource!(json = r#"{ "list": [{"uri": "1.wav"}] }"#);
        let sample = &source.list().unwrap()[0];

        set.add(&source, sample.clone()).unwrap();
        set.set_label(sample, Some(DrumkitLabel::Clap)).unwrap();
        set.remove(&source.list().unwrap()[0]).unwrap();

        assert!(set.cached_audio_hash_of(sample).is_err());
        assert!(set.get_label::<Label>(sample).is_err());
    }
}
