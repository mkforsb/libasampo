// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    errors::Error,
    prelude::StepSequenceOps,
    sequences::{NoteLength, TimeSpec},
    serialize::{TryFromDomain, TryIntoDomain, DRUMKIT_LABELS},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerV1 {
    label: String,
    amp: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrumkitSequenceV1 {
    uuid: Uuid,
    name: String,
    bpm: u16,
    time_signature_upper: u8,
    time_signature_lower: u8,
    swing: f64,
    step_base_length: String,
    steps: Vec<Vec<TriggerV1>>,
}

impl TryFromDomain<crate::sequences::DrumkitSequence> for DrumkitSequenceV1 {
    fn try_from_domain(
        value: &crate::sequences::DrumkitSequence,
    ) -> Result<Self, crate::errors::Error>
    where
        Self: Sized,
    {
        let mut steps: Vec<Vec<TriggerV1>> = Vec::new();

        for i in 0..value.len() {
            if let Some(stepinfo) = value.step(i) {
                steps.push(
                    stepinfo
                        .triggers()
                        .iter()
                        .map(|trigger| {
                            Ok(TriggerV1 {
                                label: DRUMKIT_LABELS
                                    .iter()
                                    .find(|(_s, label)| *label == trigger.label())
                                    .map(|(s, _label)| s.to_string())
                                    .ok_or(Error::SerializationError(
                                        "Unknown drumkit label".to_string(),
                                    ))?,

                                amp: trigger.amplitude(),
                            })
                        })
                        .collect::<Result<Vec<_>, Error>>()?,
                )
            } else {
                steps.push(Vec::new());
            }
        }

        Ok(DrumkitSequenceV1 {
            uuid: value.uuid(),
            name: value.name().clone(),
            bpm: value.timespec().bpm.get(),
            time_signature_upper: value.timespec().signature.upper(),
            time_signature_lower: value.timespec().signature.lower(),
            swing: value.timespec().swing.get(),
            step_base_length: match value.step_base_len() {
                crate::sequences::NoteLength::Eighth => "Eighth".to_string(),
                crate::sequences::NoteLength::Sixteenth => "Sixteenth".to_string(),
            },
            steps,
        })
    }
}

impl TryIntoDomain<crate::sequences::DrumkitSequence> for DrumkitSequenceV1 {
    fn try_into_domain(self) -> Result<crate::sequences::DrumkitSequence, Error> {
        let mut result = crate::sequences::DrumkitSequence::new_named(
            self.name,
            TimeSpec::new_with_swing(
                self.bpm,
                self.time_signature_upper,
                self.time_signature_lower,
                self.swing,
            )?,
            match self.step_base_length.as_str() {
                "Eighth" => Ok(NoteLength::Eighth),
                "Sixteenth" => Ok(NoteLength::Sixteenth),
                _ => Err(Error::DeserializationError(
                    "Unknown step base length".to_string(),
                )),
            }?,
        );

        result.set_uuid(self.uuid);
        result.set_len(self.steps.len());

        for i in 0..self.steps.len() {
            for trigger in &self.steps[i] {
                result.set_step_trigger(
                    i,
                    *DRUMKIT_LABELS
                        .iter()
                        .find(|(s, _label)| *s == trigger.label)
                        .map(|(_s, label)| label)
                        .ok_or(Error::DeserializationError(
                            "Unknown drumkit label".to_string(),
                        ))?,
                    trigger.amp,
                )
            }
        }

        Ok(result)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Sequence {
    DrumkitSequenceV1(DrumkitSequenceV1),
}

impl TryIntoDomain<crate::sequences::DrumkitSequence> for Sequence {
    fn try_into_domain(self) -> Result<crate::sequences::DrumkitSequence, Error> {
        match self {
            Sequence::DrumkitSequenceV1(seq) => seq.try_into_domain(),
        }
    }
}

impl TryFromDomain<crate::sequences::DrumkitSequence> for Sequence {
    fn try_from_domain(value: &crate::sequences::DrumkitSequence) -> Result<Self, Error> {
        Ok(Sequence::DrumkitSequenceV1(
            DrumkitSequenceV1::try_from_domain(value)?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use crate::{samplesets::DrumkitLabel, sequences::DrumkitSequence};

    use super::*;

    #[test]
    fn test_serialize_deserialize() {
        let mut sequence = DrumkitSequence::new_named(
            "Amazing Sequence",
            TimeSpec::new_with_swing(133, 3, 4, 0.22).unwrap(),
            NoteLength::Sixteenth,
        );

        sequence.set_len(20);

        sequence.set_step_trigger(3, DrumkitLabel::BassDrum, 0.75);
        sequence.set_step_trigger(10, DrumkitLabel::SnareDrum, 0.5);
        sequence.set_step_trigger(10, DrumkitLabel::OpenHihat, 0.25);
        sequence.set_step_trigger(18, DrumkitLabel::RideCymbal, 0.65);

        let serializable = DrumkitSequenceV1::try_from_domain(&sequence).unwrap();
        let serialized = serde_json::to_string_pretty(&serializable).unwrap();

        let deserialized = serde_json::from_str::<DrumkitSequenceV1>(&serialized).unwrap();
        let domained = deserialized.try_into_domain().unwrap();

        assert_eq!(format!("{sequence:?}"), format!("{domained:?}"));
    }
}
