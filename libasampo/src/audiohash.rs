// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use md5::{Digest, Md5};
use symphonia::core::{
    io::{MediaSourceStream, ReadOnlySource},
    probe::Hint,
};

use crate::{errors::Error, sources::SourceReader};

/// Compute the MD5 hash checksum of the audio data (i.e excluding any headers) in a
/// given audio source.
pub fn audio_hash(reader: SourceReader) -> Result<String, Error> {
    let mss = MediaSourceStream::new(Box::new(ReadOnlySource::new(reader)), Default::default());

    match symphonia::default::get_probe().format(
        &Hint::new(),
        mss,
        &Default::default(),
        &Default::default(),
    ) {
        Ok(probed) => {
            let track_id = probed
                .format
                .default_track()
                .ok_or(Error::SymphoniaNoDefaultTrackError)?
                .id;

            let mut reader = probed.format;
            let mut hasher = Md5::new();

            loop {
                match reader.next_packet() {
                    Ok(packet) if packet.track_id() == track_id => hasher.update(packet.data),
                    Ok(_) => continue,

                    // TODO: determine if we got the entire stream or not
                    Err(_) => break,
                }
            }

            Ok(format!("{:x}", hasher.finalize()))
        }
        Err(e) => Err(Error::SymphoniaError(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use std::{env, fs::File};

    use super::*;

    #[test]
    fn test_audio_hash() {
        // try `tail -c +45 square_1ch_48k_20smp.wav | md5sum`
        assert_eq!(
            audio_hash(SourceReader::FileReader(
                File::open(format!(
                    "{}/test_assets/square_1ch_48k_20smp.wav",
                    env::var("CARGO_MANIFEST_DIR").unwrap()
                ))
                .unwrap()
            ))
            .unwrap(),
            "82f079b6579bc527467abaf3a6d3a192".to_string()
        );
    }

    #[test]
    fn test_audio_hash_invalid() {
        assert!(audio_hash(SourceReader::VecReader(vec![], 0)).is_err());
    }
}
