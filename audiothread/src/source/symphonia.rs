// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use std::{
    fs::File,
    io::{BufReader, Read, Seek},
};

use ringbuf::{
    traits::{Consumer, Observer, Producer},
    HeapRb,
};
use symphonia::core::{
    audio::{AudioBufferRef as SymphoniaAudioBufferRef, SampleBuffer as SymphoniaSampleBuffer},
    codecs::Decoder as SymphoniaDecoder,
    formats::FormatReader as SymphoniaFormatReader,
    io::{MediaSource as SymphoniaMediaSource, MediaSourceStream as SymphoniaMediaSourceStream},
    probe::Hint as SymphoniaProbeHint,
};
use thiserror::Error as ThisError;

use crate::{
    error::{error_enum, IOError, SymphoniaError, SymphoniaSourceError, ValueOutOfRangeError},
    ext::Frames,
    source::SourceOps,
    types::{AudioSpec, StreamState},
};

pub struct SymphoniaSource {
    spec: AudioSpec,
    stream_state: StreamState,
    reader: Box<dyn SymphoniaFormatReader>,
    decoder: Box<dyn SymphoniaDecoder>,
    track_id: u32,
    buffer: HeapRb<f32>,
}

impl std::fmt::Debug for SymphoniaSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!(
            "SymphoniaSource(spec: {:?}, stream_state: {:?}, codec: {:?}, track_id: {}, \
                buffer: {} of {})",
            self.spec,
            self.stream_state,
            self.decoder.codec_params(),
            self.track_id,
            self.buffer.occupied_len(),
            self.buffer.capacity(),
        ))
    }
}

error_enum!(
    SymphoniaSourceImplError = {
        SymphoniaError,
        SymphoniaSourceError,
        ValueOutOfRangeError,
        IOError
    }
    where
        SymphoniaError from symphonia::core::errors::Error,
        IOError from std::io::Error
);

impl SymphoniaSource {
    pub fn new(
        reader: Box<dyn SymphoniaFormatReader>,
        decoder: Box<dyn SymphoniaDecoder>,
        track_id: u32,
    ) -> Result<Self, SymphoniaSourceImplError> {
        let spec = AudioSpec::new(
            decoder
                .codec_params()
                .sample_rate
                .ok_or(SymphoniaSourceError(
                    "Unable to extract audio spec (missing samplerate)".to_string(),
                ))?,
            decoder
                .codec_params()
                .channels
                .ok_or(SymphoniaSourceError(
                    "Unable to extract audio spec (missing channel count)".to_string(),
                ))?
                .count() as u8,
        )?;

        Ok(Self {
            spec,
            stream_state: StreamState::Streaming,
            reader,
            decoder,
            track_id,
            buffer: HeapRb::new(spec.channels.get() as usize * spec.samplerate.get() as usize),
        })
    }

    pub fn from_file(path: &str) -> Result<SymphoniaSource, SymphoniaSourceImplError> {
        Self::from_buf_reader(BufReader::new(File::open(path)?))
    }

    pub fn from_buf_reader<R: Read + Seek + Send + Sync + 'static>(
        mut bufreader: BufReader<R>,
    ) -> Result<SymphoniaSource, SymphoniaSourceImplError> {
        let len = bufreader.seek(std::io::SeekFrom::End(0)).ok();
        let _ = bufreader.seek(std::io::SeekFrom::Start(0));

        let mss = SymphoniaMediaSourceStream::new(
            Box::new(BufReadWrap { bufreader, len }),
            Default::default(),
        );

        match symphonia::default::get_probe().format(
            &SymphoniaProbeHint::new(),
            mss,
            &Default::default(),
            &Default::default(),
        ) {
            Ok(probed) => {
                let codecs = symphonia::default::get_codecs();
                let track_id: u32 = probed
                    .format
                    .default_track()
                    .ok_or(SymphoniaSourceError("No default track".to_string()))?
                    .id;
                let codec_params = &probed
                    .format
                    .default_track()
                    .ok_or(SymphoniaSourceError("No default track".to_string()))?
                    .codec_params;
                let decoder = codecs.make(codec_params, &Default::default())?;

                Ok(SymphoniaSource::new(probed.format, decoder, track_id)?)
            }
            Err(e) => Err(e.into()),
        }
    }

    fn decode_next_packet(&mut self) -> Option<SymphoniaAudioBufferRef> {
        let mut packet = self.reader.next_packet().ok()?;

        while packet.track_id() != self.track_id {
            packet = self.reader.next_packet().ok()?;
        }

        match self.decoder.decode(&packet) {
            Ok(audiobuf) => Some(audiobuf),
            Err(_) => None,
        }
    }
}

impl SourceOps for SymphoniaSource {
    fn spec(&self) -> AudioSpec {
        self.spec
    }

    fn stream_state(&self) -> crate::types::StreamState {
        self.stream_state
    }

    fn mix_to_same_spec(&mut self, out_buffer: &mut [f32]) {
        let self_chans = self.spec.channels.get() as usize;
        let num_out_buffer_frames = out_buffer.len_frames(self.spec).get();

        debug_assert!(self.buffer.occupied_len() % self_chans == 0);

        let prior_decoded_frames_avail = self.buffer.occupied_len() / self_chans;
        let prior_decoded_frames_drained =
            std::cmp::min(num_out_buffer_frames, prior_decoded_frames_avail);

        out_buffer
            .iter_mut()
            .zip(self.buffer.pop_iter())
            .for_each(|(output, sample)| *output += sample);

        let mut out_buffer_frame_offset = prior_decoded_frames_drained;
        let mut frames_needed = num_out_buffer_frames - prior_decoded_frames_drained;

        while frames_needed > 0 {
            match self.decode_next_packet() {
                Some(audiobuf) => {
                    let mut samplebuf = SymphoniaSampleBuffer::<f32>::new(
                        audiobuf.capacity() as u64,
                        *audiobuf.spec(),
                    );
                    samplebuf.copy_interleaved_ref(audiobuf);

                    debug_assert!(samplebuf.len() % self_chans == 0);

                    let num_decoded_frames = samplebuf.len() / self_chans;
                    let num_decoded_frames_mixed = std::cmp::min(frames_needed, num_decoded_frames);

                    out_buffer
                        .slice_frames_mut(self.spec, out_buffer_frame_offset..)
                        .iter_mut()
                        .zip(samplebuf.samples().iter())
                        .for_each(|(output, sample)| *output += sample);

                    if num_decoded_frames - num_decoded_frames_mixed > 0 {
                        self.buffer.push_slice(
                            samplebuf
                                .samples()
                                .slice_frames(self.spec, num_decoded_frames_mixed..),
                        );
                    }

                    out_buffer_frame_offset += num_decoded_frames_mixed;
                    frames_needed -= num_decoded_frames_mixed;
                }
                None => {
                    self.stream_state = StreamState::Complete;
                    break;
                }
            }
        }
    }
}

struct BufReadWrap<R: Read + Seek + Send + Sync> {
    bufreader: BufReader<R>,
    len: Option<u64>,
}

impl<R: Read + Seek + Send + Sync> Read for BufReadWrap<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.bufreader.read(buf)
    }
}

impl<R: Read + Seek + Send + Sync> Seek for BufReadWrap<R> {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.bufreader.seek(pos)
    }
}

impl<R: Read + Seek + Send + Sync> SymphoniaMediaSource for BufReadWrap<R> {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        self.len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symphoniafile_eof() {
        let mut sf = SymphoniaSource::from_file(&format!(
            "{}/test_assets/square_1ch_48k_20smp.wav",
            std::env::var("CARGO_MANIFEST_DIR").unwrap()
        ))
        .unwrap();

        let mut buf = [0.0f32; 21];

        sf.mix_to_same_spec(&mut buf);

        assert_eq!(sf.stream_state(), StreamState::Complete);
    }
}
