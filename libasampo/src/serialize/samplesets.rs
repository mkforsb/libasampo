// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    errors::Error,
    prelude::ConcreteSampleSetLabelling,
    samples::SampleOps,
    samplesets::{SampleSetLabelling, SampleSetOps},
    serialize::{TryFromDomain, TryIntoDomain},
};

const DRUMKIT_LABELS: [(&str, crate::samplesets::DrumkitLabel); 16] = [
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseSampleSetV1 {
    uuid: Uuid,
    name: String,
    samples: Vec<crate::serialize::samples::Sample>,
    labelling_kind: String,
    labels: Option<Vec<Option<String>>>,
    audio_hash: Vec<String>,
}

impl TryIntoDomain<crate::samplesets::BaseSampleSet> for BaseSampleSetV1 {
    fn try_into_domain(self) -> Result<crate::samplesets::BaseSampleSet, Error> {
        let mut result = crate::samplesets::BaseSampleSet::new(self.name);
        result.set_uuid(self.uuid);

        let samples = self
            .samples
            .into_iter()
            .map(|x| x.try_into_domain())
            .collect::<Result<Vec<_>, Error>>()?;

        for (i, sample) in samples.iter().enumerate() {
            result.add_with_hash(
                sample.clone(),
                self.audio_hash
                    .get(i)
                    .cloned()
                    .ok_or(Error::DeserializationError(
                        "Serialized sample set missing audio hash for sample".to_string(),
                    ))?,
            );
        }

        if self.labelling_kind.as_str() == "drumkit" {
            let labels = self.labels.expect(
                "Serialized sample set with labelling_kind != none should contain \
                    list of labels",
            );

            if labels.len() < samples.len() {
                return Err(Error::DeserializationError(
                    "Serialized sample set missing labels".to_string(),
                ));
            }

            let mut labelling = crate::samplesets::DrumkitLabelling::new();

            for (i, sample) in samples.iter().enumerate() {
                match labels.get(i).unwrap() {
                    Some(label) => labelling.set(
                        sample.uri().clone(),
                        DRUMKIT_LABELS
                            .iter()
                            .find(|(s, _val)| s == label)
                            .map(|(_s, val)| *val)
                            .ok_or(Error::DeserializationError(
                                "Unknown drumkit label".to_string(),
                            ))?,
                    ),
                    None => (),
                }
            }

            result.set_labelling(Some(SampleSetLabelling::DrumkitLabelling(labelling)));
        }

        Ok(result)
    }
}

impl TryFromDomain<crate::samplesets::BaseSampleSet> for BaseSampleSetV1 {
    fn try_from_domain(value: &crate::samplesets::BaseSampleSet) -> Result<Self, Error> {
        let uuid = *value.uuid();
        let name = value.name().to_string();
        let samples = value
            .list()
            .iter()
            .map(|x| (*x).clone())
            .collect::<Vec<_>>();

        let (labelling_kind, labels) = match value.labelling() {
            Some(SampleSetLabelling::DrumkitLabelling(labels)) => ("drumkit".to_string(), {
                Some(
                    samples
                        .iter()
                        .map(|sample| {
                            labels.get(sample.uri()).and_then(|label| {
                                DRUMKIT_LABELS
                                    .iter()
                                    .find(|(_s, val)| val == label)
                                    .map(|(s, _val)| s.to_string())
                            })
                        })
                        .collect(),
                )
            }),
            None => ("none".to_string(), None),
        };

        let audio_hash = samples
            .iter()
            .map(|x| {
                value.cached_audio_hash_of(x).map(|x| x.to_string()).ok_or(
                    Error::SerializationError(
                        "SampleSet missing audio hash for sample".to_string(),
                    ),
                )
            })
            .collect::<Result<Vec<_>, Error>>()?;

        let samples = samples
            .iter()
            .map(crate::serialize::Sample::try_from_domain)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(BaseSampleSetV1 {
            uuid,
            name,
            samples,
            labelling_kind,
            labels,
            audio_hash,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SampleSet {
    BaseSampleSetV1(BaseSampleSetV1),
}

impl TryIntoDomain<crate::samplesets::SampleSet> for SampleSet {
    fn try_into_domain(self) -> Result<crate::samplesets::SampleSet, Error> {
        match self {
            SampleSet::BaseSampleSetV1(set) => Ok(crate::samplesets::SampleSet::BaseSampleSet(
                set.try_into_domain()?,
            )),
        }
    }
}

impl TryFromDomain<crate::samplesets::SampleSet> for SampleSet {
    fn try_from_domain(value: &crate::samplesets::SampleSet) -> Result<Self, Error> {
        match value {
            crate::samplesets::SampleSet::BaseSampleSet(set) => Ok(SampleSet::BaseSampleSetV1(
                BaseSampleSetV1::try_from_domain(set)?,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        prelude::{SampleSetLabellingOps, SourceOps},
        samplesets::{DrumkitLabel, DrumkitLabelling},
        testutils::{audiohash_for_test, s},
    };

    use super::*;

    #[test]
    fn test_basesampleset() {
        audiohash_for_test::RESULT.set(Some(|_| Ok("hashresponse".to_string())));

        use crate::testutils::{fakesource, fakesource_from_json};

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

        let mut set = crate::samplesets::BaseSampleSet::new(s("Favorites"));

        set.add(&src, s1.clone()).unwrap();
        set.add(&src, s2.clone()).unwrap();

        set.set_labelling(Some(SampleSetLabelling::DrumkitLabelling(
            DrumkitLabelling::new(),
        )));

        match set.labelling_mut() {
            Some(SampleSetLabelling::DrumkitLabelling(labels)) => {
                labels.set(s1.uri().clone(), DrumkitLabel::CrashCymbal);
            }
            None => panic!(),
        }

        let serializable =
            SampleSet::try_from_domain(&crate::samplesets::SampleSet::BaseSampleSet(set)).unwrap();

        let encoded = serde_json::to_string_pretty(&serializable).unwrap();
        let decoded = serde_json::from_str::<SampleSet>(&encoded).unwrap();

        match &decoded {
            SampleSet::BaseSampleSetV1(set) => {
                assert_eq!(set.name, "Favorites");
                assert_eq!(set.samples.len(), 2);
                assert_eq!(set.labels.as_ref().unwrap().len(), 2);
                assert_eq!(
                    set.audio_hash.first(),
                    Some("hashresponse".to_string()).as_ref()
                );
                assert_eq!(
                    set.audio_hash.get(1),
                    Some("hashresponse".to_string()).as_ref()
                );
            }

            #[allow(unreachable_patterns)]
            _ => panic!(),
        }

        let domained = decoded.try_into_domain().unwrap();

        match &domained {
            crate::samplesets::SampleSet::BaseSampleSet(set) => {
                assert_eq!(set.name(), "Favorites");
                assert_eq!(set.len(), 2);
                assert!(set.contains(s1));
                assert!(set.contains(s2));
                assert_eq!(set.list().len(), 2);
                assert!(set.list().contains(&s1));
                assert!(set.list().contains(&s2));
                assert_eq!(set.labelling().unwrap().len(), 1);
                assert!(set.labelling().unwrap().contains(s1.uri()));
                assert!(!set.labelling().unwrap().contains(s2.uri()));
                assert_eq!(set.cached_audio_hash_of(s1), Some("hashresponse"));
                assert_eq!(set.cached_audio_hash_of(s2), Some("hashresponse"));
            }

            #[allow(unreachable_patterns)]
            _ => panic!(),
        }
    }
}
