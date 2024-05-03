// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use libasampo::{
    samples::{BaseSample, Sample, SampleMetadata, SampleURI},
    serialize::{deserialize, serialize},
    sources::{file_system_source::FilesystemSource, Source},
};
use uuid::Uuid;

#[test]
fn test_serialize_sample() {
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

    let x = serialize(&sample).unwrap();
    assert_eq!(deserialize::<Sample>(x).unwrap(), sample);
}

#[test]
fn test_serialize_fs_source() {
    let source = Source::FilesystemSource(FilesystemSource::new(
        "/tmp".to_string(),
        vec!["wav".to_string(), "ogg".to_string()],
    ));

    let x = serialize(&source).unwrap();
    assert_eq!(deserialize::<Source>(x).unwrap(), source);
}
