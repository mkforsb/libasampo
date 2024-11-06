// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    audiohash::AudioHasher,
    errors::Error,
    samplesets::{
        BaseSampleSet as DomBaseSampleSet, DrumkitLabel, Label, SampleSet as DomSampleSet,
        SampleSetOps,
    },
    serialize::{samples::Sample as SerSample, TryFromDomain, TryIntoDomain},
};

pub const DRUMKIT_LABELS: [(&str, crate::samplesets::DrumkitLabel); 16] = [
    ("RimShot", crate::samplesets::DrumkitLabel::RimShot),
    ("Clap", crate::samplesets::DrumkitLabel::Clap),
    ("ClosedHihat", crate::samplesets::DrumkitLabel::ClosedHihat),
    ("OpenHihat", crate::samplesets::DrumkitLabel::OpenHihat),
    ("CrashCymbal", crate::samplesets::DrumkitLabel::CrashCymbal),
    ("RideCymbal", crate::samplesets::DrumkitLabel::RideCymbal),
    ("Shaker", crate::samplesets::DrumkitLabel::Shaker),
    ("BassDrum", crate::samplesets::DrumkitLabel::BassDrum),
    ("SnareDrum", crate::samplesets::DrumkitLabel::SnareDrum),
    ("LowTom", crate::samplesets::DrumkitLabel::LowTom),
    ("MidTom", crate::samplesets::DrumkitLabel::MidTom),
    ("HighTom", crate::samplesets::DrumkitLabel::HighTom),
    ("Perc1", crate::samplesets::DrumkitLabel::Perc1),
    ("Perc2", crate::samplesets::DrumkitLabel::Perc2),
    ("Perc3", crate::samplesets::DrumkitLabel::Perc3),
    ("Perc4", crate::samplesets::DrumkitLabel::Perc4),
];

fn key_for(label: DrumkitLabel) -> Option<&'static str> {
    DRUMKIT_LABELS
        .iter()
        .find(|(_key, val)| *val == label)
        .map(|(key, _val)| *key)
}

fn label_for(key: &str) -> Option<DrumkitLabel> {
    DRUMKIT_LABELS
        .iter()
        .find(|(k, _val)| *k == key)
        .map(|(_k, val)| *val)
}

fn substr(s: &str, start: usize, len: usize) -> String {
    let mut result: Box<dyn Iterator<Item = char>> = Box::new(s.chars());

    if start > 0 {
        result = Box::new(result.skip(start));
    }

    if len > 0 {
        result = Box::new(result.take(len));
    }

    result.collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryV1 {
    sample: SerSample,
    label: Option<String>,
    audio_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseSampleSetV1 {
    uuid: Uuid,
    name: String,
    samples: Vec<EntryV1>,
}

impl TryIntoDomain<DomBaseSampleSet> for BaseSampleSetV1 {
    fn try_into_domain(self) -> Result<DomBaseSampleSet, Error> {
        let mut result = DomBaseSampleSet::new(self.name);
        result.set_uuid(self.uuid);

        for entry in self.samples {
            let sample = entry.sample.try_into_domain()?;

            result.add_with_hash(sample.clone(), entry.audio_hash);

            result.set_label(
                &sample,
                if let Some(text) = entry.label {
                    if text.starts_with("DrumkitLabel.") {
                        Some(Label::DrumkitLabel(
                            label_for(&substr(&text, 13, 0))
                                .ok_or(Error::DeserializationError("Unknown label".to_string()))?,
                        ))
                    } else {
                        Err(Error::DeserializationError("Unknown label".to_string()))?
                    }
                } else {
                    None
                },
            )?;
        }

        Ok(result)
    }
}

impl<H> TryFromDomain<DomBaseSampleSet<H>> for BaseSampleSetV1
where
    H: AudioHasher,
{
    fn try_from_domain(set: &DomBaseSampleSet<H>) -> Result<Self, Error> {
        let uuid = set.uuid();
        let name = set.name().to_string();
        let samples = set
            .list()
            .iter()
            .map(|sample| -> Result<EntryV1, Error> {
                let label = set.get_label::<Label>(sample)?;
                let audio_hash = set.cached_audio_hash_of(sample)?.to_string();

                Ok(EntryV1 {
                    sample: SerSample::try_from_domain(sample)?,
                    label: match label {
                        Some(Label::DrumkitLabel(label)) => Some(format!(
                            "DrumkitLabel.{}",
                            key_for(label)
                                .ok_or(Error::SerializationError("Unknown label".to_string()))?
                        )),
                        None => None,
                    },
                    audio_hash,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;

        Ok(BaseSampleSetV1 {
            uuid,
            name,
            samples,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SampleSet {
    BaseSampleSetV1(BaseSampleSetV1),
}

impl TryIntoDomain<DomSampleSet> for SampleSet {
    fn try_into_domain(self) -> Result<DomSampleSet, Error> {
        match self {
            SampleSet::BaseSampleSetV1(set) => {
                Ok(DomSampleSet::BaseSampleSet(set.try_into_domain()?))
            }
        }
    }
}

impl<H> TryFromDomain<DomSampleSet<H>> for SampleSet
where
    H: AudioHasher,
{
    fn try_from_domain(value: &DomSampleSet<H>) -> Result<Self, Error> {
        match value {
            DomSampleSet::BaseSampleSet(set) => Ok(SampleSet::BaseSampleSetV1(
                BaseSampleSetV1::try_from_domain(set)?,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        audiohash::AudioHasher, samplesets::DrumkitLabel, sources::SourceOps, testutils::fakesource,
    };

    use super::*;

    #[test]
    fn test_basesampleset() {
        #[derive(Debug, Clone, PartialEq, Eq)]
        struct DummyHasher;

        impl AudioHasher for DummyHasher {
            fn audio_hash(_reader: crate::sources::SourceReader) -> Result<String, Error> {
                Ok("hashresponse".to_string())
            }
        }

        let src = fakesource!(
            json = r#"{
            "list": [
                {"uri": "file:///tmp/1.wav", "name": "1.wav" },
                {"uri": "file:///tmp/2.wav", "name": "2.wav" }
            ]
            }"#
        );

        let samples = src.list().unwrap();
        let s1 = samples.first().unwrap();
        let s2 = samples.get(1).unwrap();

        let mut set = DomBaseSampleSet::new_with_hasher::<DummyHasher>("Favorites");

        set.add(&src, s1.clone()).unwrap();
        set.add(&src, s2.clone()).unwrap();

        set.set_label(s1, Some(DrumkitLabel::CrashCymbal)).unwrap();

        let serializable = SampleSet::try_from_domain(&DomSampleSet::BaseSampleSet(set)).unwrap();

        let encoded = serde_json::to_string_pretty(&serializable).unwrap();
        let decoded = serde_json::from_str::<SampleSet>(&encoded).unwrap();

        match &decoded {
            SampleSet::BaseSampleSetV1(set) => {
                assert_eq!(set.name, "Favorites");
                assert_eq!(set.samples.len(), 2);
                assert_eq!(set.samples.first().unwrap().audio_hash, "hashresponse");
                assert_eq!(set.samples.get(1).unwrap().audio_hash, "hashresponse");
            }

            #[allow(unreachable_patterns)]
            _ => panic!(),
        }

        let domained = decoded.try_into_domain().unwrap();

        match &domained {
            DomSampleSet::BaseSampleSet(set) => {
                assert_eq!(set.name(), "Favorites");
                assert_eq!(set.len(), 2);
                assert!(set.contains(s1));
                assert!(set.contains(s2));
                assert_eq!(set.list().len(), 2);
                assert!(set.list().contains(&s1));
                assert!(set.list().contains(&s2));
                assert_eq!(set.cached_audio_hash_of(s1).unwrap(), "hashresponse");
                assert_eq!(set.cached_audio_hash_of(s2).unwrap(), "hashresponse");
            }

            #[allow(unreachable_patterns)]
            _ => panic!(),
        }
    }
}
