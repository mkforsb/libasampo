// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use symphonia::core::{
    audio::SampleBuffer,
    io::{MediaSourceStream, ReadOnlySource},
    probe::Hint,
};

use crate::{errors::Error, prelude::SourceOps, samples::Sample, sources::Source};

pub struct U8GreaterThanTwo {
    value: u8,
}

impl TryFrom<u8> for U8GreaterThanTwo {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        if value > 2 {
            Ok(U8GreaterThanTwo { value })
        } else {
            Err(Error::ValueOutOfRangeError(
                "Value must be greater than 2".to_string(),
            ))
        }
    }
}

pub enum ChannelMapping {
    Passthrough,
    MonoToStereo,
    StereoToMono,
    TruncateToStereo { input_channels: U8GreaterThanTwo },
}

pub struct RateConversion {
    pub from: u32,
    pub to: u32,
}

pub trait SampleValueConvert {
    fn convert(val: f32) -> Self;
}

impl SampleValueConvert for f32 {
    fn convert(val: f32) -> Self {
        val.clamp(-1.0, 1.0)
    }
}

impl SampleValueConvert for i32 {
    fn convert(val: f32) -> Self {
        (val.clamp(-1.0, 1.0) * (i32::MAX as f32)) as i32
    }
}

impl SampleValueConvert for i16 {
    fn convert(val: f32) -> Self {
        (val.clamp(-1.0, 1.0) * (i16::MAX as f32)) as i16
    }
}

impl SampleValueConvert for i8 {
    fn convert(val: f32) -> Self {
        (val.clamp(-1.0, 1.0) * (i8::MAX as f32)) as i8
    }
}

pub struct AudioConversionIterator<T>
where
    T: SampleValueConvert + Copy,
{
    samples: std::vec::IntoIter<f32>,
    outstack: Vec<T>,
    channel_mapping: ChannelMapping,
}

impl<T> AudioConversionIterator<T>
where
    T: SampleValueConvert + Copy,
{
    pub fn new(samples: std::vec::IntoIter<f32>, cm: ChannelMapping) -> Self {
        AudioConversionIterator {
            samples,
            outstack: Vec::new(),
            channel_mapping: cm,
        }
    }
}

impl<T> Iterator for AudioConversionIterator<T>
where
    T: SampleValueConvert + Copy,
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        if !self.outstack.is_empty() {
            self.outstack.pop()
        } else {
            match self.channel_mapping {
                ChannelMapping::Passthrough => self.samples.next().map(T::convert),
                ChannelMapping::MonoToStereo => match self.samples.next() {
                    Some(val) => {
                        let val = T::convert(val);
                        self.outstack.push(val);
                        Some(val)
                    }
                    None => None,
                },
                ChannelMapping::StereoToMono => match (self.samples.next(), self.samples.next()) {
                    (Some(a), Some(b)) => Some(T::convert((a + b) / 2.0)),
                    _ => None,
                },
                ChannelMapping::TruncateToStereo { ref input_channels } => {
                    match (self.samples.next(), self.samples.next()) {
                        (Some(a), Some(b)) => {
                            for _ in 0..(input_channels.value - 2) {
                                let _ = self.samples.next();
                            }

                            self.outstack.push(T::convert(b));
                            Some(T::convert(a))
                        }
                        _ => None,
                    }
                }
            }
        }
    }
}

pub fn convert<T>(
    samples: Vec<f32>,
    input_channels: u8,
    cm: ChannelMapping,
    rc: Option<RateConversion>,
    rcq: Option<samplerate::ConverterType>,
) -> Result<Vec<T>, Error>
where
    T: SampleValueConvert + Copy,
{
    if samples.len() % input_channels as usize != 0 {
        return Err(Error::SampleConversionError(format!(
            "Buffer length ({}) - channel count ({}) mismatch",
            samples.len(),
            input_channels
        )));
    }

    match cm {
        ChannelMapping::MonoToStereo if input_channels != 1 => Err(Error::SampleConversionError(
            "Invalid channel mapping".to_string(),
        )),
        ChannelMapping::StereoToMono if input_channels != 2 => Err(Error::SampleConversionError(
            "Invalid channel mapping".to_string(),
        )),
        _ => Ok(()),
    }?;

    let samples = match rc {
        Some(rc) if rc.from != rc.to => samplerate::convert(
            rc.from,
            rc.to,
            input_channels as usize,
            rcq.unwrap_or(samplerate::ConverterType::SincBestQuality),
            samples.as_slice(),
        )
        .map_err(|e| Error::SampleConversionError(e.to_string()))?,
        _ => samples,
    };

    Ok(AudioConversionIterator::<T>::new(samples.into_iter(), cm).collect())
}

pub fn decode(source: &Source, sample: &Sample) -> Result<Vec<f32>, Error> {
    let mut output: Vec<f32> = Vec::new();
    let mss = MediaSourceStream::new(
        Box::new(ReadOnlySource::new(source.stream(sample)?)),
        Default::default(),
    );

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

            let codec_params = &probed
                .format
                .default_track()
                .ok_or(Error::SymphoniaNoDefaultTrackError)?
                .codec_params;

            let mut decoder = symphonia::default::get_codecs()
                .make(codec_params, &Default::default())
                .map_err(|e| Error::SymphoniaError(e.to_string()))?;

            let mut reader = probed.format;

            loop {
                match reader.next_packet() {
                    Ok(packet) if packet.track_id() == track_id => match decoder.decode(&packet) {
                        Ok(audiobuf) => {
                            let mut samplebuf = SampleBuffer::<f32>::new(
                                audiobuf.capacity() as u64,
                                *audiobuf.spec(),
                            );

                            samplebuf.copy_interleaved_ref(audiobuf);
                            output.extend_from_slice(samplebuf.samples());
                        }
                        Err(_) => todo!(),
                    },

                    Ok(_) => continue,

                    // TODO: determine if we got the entire stream or not
                    Err(_) => break,
                }
            }

            Ok(output)
        }
        Err(e) => Err(Error::SymphoniaError(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use crate::convert::ChannelMapping;

    use super::*;

    #[test]
    fn test_reject_invalid_channel_mappings() {
        assert!(convert::<i16>(
            vec![1.0, 2.0, 3.0],
            2,
            ChannelMapping::MonoToStereo,
            None,
            None
        )
        .is_err());
        assert!(convert::<i16>(
            vec![1.0, 2.0, 3.0],
            1,
            ChannelMapping::StereoToMono,
            None,
            None
        )
        .is_err());
    }

    #[test]
    fn test_reject_channel_count_mismatch() {
        assert!(convert::<f32>(
            vec![1.0, 2.0, 3.0],
            2,
            ChannelMapping::Passthrough,
            None,
            None
        )
        .is_err());
    }

    #[test]
    fn test_mono_to_stereo() {
        assert_eq!(
            convert::<f32>(
                vec![0.1, 0.2, 0.3],
                1,
                ChannelMapping::MonoToStereo,
                None,
                None
            )
            .unwrap(),
            vec![0.1, 0.1, 0.2, 0.2, 0.3, 0.3]
        )
    }

    #[test]
    fn test_stereo_to_mono() {
        assert_eq!(
            convert::<f32>(
                vec![0.1, 0.1, 0.2, 0.2, 0.3, 0.3],
                2,
                ChannelMapping::StereoToMono,
                None,
                None
            )
            .unwrap(),
            vec![0.1, 0.2, 0.3]
        )
    }

    #[test]
    fn test_truncate_to_stereo() {
        assert_eq!(
            convert::<f32>(
                vec![0.1, 0.1, 0.1, 0.1, 0.1, 0.2, 0.2, 0.2, 0.2, 0.2, 0.3, 0.3, 0.3, 0.3, 0.3],
                5,
                ChannelMapping::TruncateToStereo {
                    input_channels: 5.try_into().unwrap()
                },
                None,
                None
            )
            .unwrap(),
            vec![0.1, 0.1, 0.2, 0.2, 0.3, 0.3]
        )
    }

    #[test]
    fn test_to_i32() {
        assert!(convert::<i32>(
            vec![1.0, 0.0, -1.0],
            1,
            ChannelMapping::Passthrough,
            None,
            None
        )
        .unwrap()
        .iter()
        .zip([i32::MAX, 0, i32::MIN].iter())
        .map(|(a, b)| (a - b).abs())
        .all(|x| x < 2),);
    }

    #[test]
    fn test_to_i16() {
        assert!(convert::<i16>(
            vec![1.0, 0.0, -1.0],
            1,
            ChannelMapping::Passthrough,
            None,
            None
        )
        .unwrap()
        .iter()
        .zip([i16::MAX, 0, i16::MIN].iter())
        .map(|(a, b)| (a - b).abs())
        .all(|x| x < 2),);
    }

    #[test]
    fn test_to_i8() {
        assert!(convert::<i8>(
            vec![1.0, 0.0, -1.0],
            1,
            ChannelMapping::Passthrough,
            None,
            None
        )
        .unwrap()
        .iter()
        .zip([i8::MAX, 0, i8::MIN].iter())
        .map(|(a, b)| (a - b).abs())
        .all(|x| x < 2),);
    }

    #[test]
    fn test_rate_conversion() {
        assert_eq!(
            convert::<i16>(
                (0..44100).map(|x| x as f32).collect(),
                1,
                ChannelMapping::Passthrough,
                Some(RateConversion {
                    from: 44100,
                    to: 48000
                }),
                Some(samplerate::ConverterType::SincBestQuality)
            )
            .unwrap()
            .len(),
            48000
        );

        assert_eq!(
            convert::<i16>(
                (0..96000).map(|x| x as f32).collect(),
                1,
                ChannelMapping::Passthrough,
                Some(RateConversion {
                    from: 96000,
                    to: 22500
                }),
                Some(samplerate::ConverterType::Linear)
            )
            .unwrap()
            .len(),
            22500
        );
    }
}
