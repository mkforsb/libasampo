// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use libasampo::{
    samples::{BaseSample, Sample, SampleMetadata, SampleURI},
    serialize::{self, TryFromDomain, TryIntoDomain},
    sources::{file_system_source::FilesystemSource, Source},
};
use uuid::Uuid;

#[test]
fn test_ser_de_sample() {
    let source_uuid = Uuid::new_v4();

    let sample = Sample::BaseSample(BaseSample::new(
        &SampleURI("file:///tmp/sound.wav".to_string()),
        "sound.wav",
        &SampleMetadata {
            rate: 44100,
            channels: 2,
            src_fmt_display: "PCM".to_string(),
        },
        Some(source_uuid),
    ));

    let serdeable = serialize::Sample::try_from_domain(&sample).unwrap();
    let encoded = serde_json::to_string(&serdeable).unwrap();
    let decoded = serde_json::from_str::<serialize::Sample>(&encoded).unwrap();
    let deserialized = decoded.try_into_domain().unwrap();
    assert_eq!(deserialized, sample);
}

#[test]
fn test_ser_de_fs_source() {
    let source = Source::FilesystemSource(FilesystemSource::new(
        "/tmp".to_string(),
        vec!["wav".to_string(), "ogg".to_string()],
    ));

    let serdeable = serialize::Source::try_from_domain(&source).unwrap();
    let encoded = serde_json::to_string(&serdeable).unwrap();
    let decoded = serde_json::from_str::<serialize::Source>(&encoded).unwrap();
    let deserialized = decoded.try_into_domain().unwrap();
    assert_eq!(deserialized, source);
}
