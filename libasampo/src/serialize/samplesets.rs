// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    errors::Error,
    samples::SampleOps,
    samplesets::{SampleSetLabelling, SampleSetOps},
    serialize::{TryFromDomain, TryIntoDomain},
};

const DRUMKIT_LABELS: [(&str, crate::samplesets::DrumkitLabel); 11] = [
    ("BassDrum", crate::samplesets::DrumkitLabel::BassDrum),
    ("Rimshot", crate::samplesets::DrumkitLabel::Rimshot),
    ("Snare", crate::samplesets::DrumkitLabel::Snare),
    ("Clap", crate::samplesets::DrumkitLabel::Clap),
    ("ClosedHihat", crate::samplesets::DrumkitLabel::ClosedHihat),
    ("OpenHihat", crate::samplesets::DrumkitLabel::OpenHihat),
    ("CrashCymbal", crate::samplesets::DrumkitLabel::CrashCymbal),
    ("RideCymbal", crate::samplesets::DrumkitLabel::RideCymbal),
    ("Tom1", crate::samplesets::DrumkitLabel::Tom1),
    ("Tom2", crate::samplesets::DrumkitLabel::Tom2),
    ("Tom3", crate::samplesets::DrumkitLabel::Tom3),
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
        let mut result = crate::samplesets::BaseSampleSet::new(&self.name);
        result.set_uuid(self.uuid);

        let samples = self
            .samples
            .into_iter()
            .map(|x| x.try_into_domain())
            .collect::<Result<Vec<_>, Error>>()?;

        for (i, sample) in samples.iter().enumerate() {
            result.add_with_hash(
                sample,
                self.audio_hash.get(i).ok_or(Error::DeserializationError(
                    "Serialized sample set missing audio hash for sample".to_string(),
                ))?,
            );
        }

        if self.labelling_kind.as_str() == "drumkit" {
            let labels = self.labels.expect(
                "Serialized sample set with labelling_kind != none should contain\
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
                        sample.uri(),
                        DRUMKIT_LABELS
                            .iter()
                            .find(|(s, _val)| s == label)
                            .map(|(_s, val)| val.clone())
                            .ok_or(Error::DeserializationError(
                                "Unknown drumkit label".to_string(),
                            ))?,
                    ),
                    None => (),
                }
            }
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
                samples
                    .iter()
                    .map(|sample| {
                        labels.get(sample.uri()).map(|label| {
                            DRUMKIT_LABELS
                                .iter()
                                .find(|(_s, val)| val == label)
                                .map(|(s, _val)| s.to_string())
                        })
                    })
                    .collect()
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
    use crate::{prelude::SourceOps, testutils::audiohash_for_test};

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

        let mut set = crate::samplesets::BaseSampleSet::new("Roliga Ljud");

        set.add(&src, src.list().unwrap().first().unwrap()).unwrap();
        set.add(&src, src.list().unwrap().get(1).unwrap()).unwrap();

        let serializable =
            SampleSet::try_from_domain(&crate::samplesets::SampleSet::BaseSampleSet(set)).unwrap();

        let encoded = serde_json::to_string(&serializable).unwrap();
        let decoded = serde_json::from_str::<SampleSet>(&encoded).unwrap();

        let domained = decoded.try_into_domain().unwrap();
    }
}