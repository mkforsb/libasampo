// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use ringbuf::{
    traits::{Consumer, Observer, Producer},
    HeapRb,
};
use samplerate::{ConverterType, Samplerate as SamplerateConverter};

use crate::{
    error::MismatchedSpecError,
    ext::{BufferIteratorOps, Frames},
    types::{AudioSpec, NumChannels, Quality, StreamState},
};

pub(crate) mod pulled;
pub(crate) mod symphonia;

use pulled::PulledSource;
use symphonia::SymphoniaSource;

pub(crate) trait SourceOps {
    fn spec(&self) -> AudioSpec;
    fn stream_state(&self) -> StreamState;
    fn mix_to_same_spec(&mut self, buffer: &mut [f32]);
}

#[derive(Debug)]
pub(crate) struct FakeSource {
    spec: AudioSpec,
    buffer: Vec<f32>,
    stream_state: StreamState,
    read_frame_offset: usize,
}

impl SourceOps for FakeSource {
    fn spec(&self) -> AudioSpec {
        self.spec
    }

    fn stream_state(&self) -> StreamState {
        self.stream_state
    }

    fn mix_to_same_spec(&mut self, out_buffer: &mut [f32]) {
        let self_offset_buffer = self
            .buffer
            .slice_frames(self.spec, self.read_frame_offset..);

        out_buffer
            .iter_mut()
            .zip(self_offset_buffer)
            .for_each(|(output, sample)| *output += sample);

        self.read_frame_offset += std::cmp::min(
            out_buffer.len_frames(self.spec),
            self_offset_buffer.len_frames(self.spec),
        )
        .get();

        debug_assert!(self.read_frame_offset <= self.buffer.len_frames(self.spec).get());

        if self.read_frame_offset == self.buffer.len_frames(self.spec).get() {
            self.stream_state = StreamState::Complete;
        }
    }
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant, clippy::enum_variant_names)]
pub(crate) enum Source {
    #[cfg(test)]
    #[allow(dead_code)]
    FakeSource(FakeSource),

    SymphoniaSource(SymphoniaSource),
    PulledSource(PulledSource),
}

impl SourceOps for Source {
    fn spec(&self) -> AudioSpec {
        match self {
            #[cfg(test)]
            Source::FakeSource(source) => source.spec(),

            Source::SymphoniaSource(source) => source.spec(),
            Source::PulledSource(source) => source.spec(),
        }
    }

    fn stream_state(&self) -> StreamState {
        match self {
            #[cfg(test)]
            Source::FakeSource(source) => source.stream_state(),

            Source::SymphoniaSource(source) => source.stream_state(),
            Source::PulledSource(source) => source.stream_state(),
        }
    }

    fn mix_to_same_spec(&mut self, buffer: &mut [f32]) {
        match self {
            #[cfg(test)]
            Source::FakeSource(source) => source.mix_to_same_spec(buffer),

            Source::SymphoniaSource(source) => source.mix_to_same_spec(buffer),
            Source::PulledSource(source) => source.mix_to_same_spec(buffer),
        }
    }
}

enum ChannelConversion {
    DuplicateAndOrTruncate {
        input_channels: NumChannels,
        output_channels: NumChannels,
    },
}

pub(crate) struct SourceGroup {
    spec: AudioSpec,
    sources: Vec<Source>,
    channel_conv: Option<ChannelConversion>,
    samplerate_conv: Option<samplerate::samplerate::Samplerate>,
    samplerate_conv_done_once: bool,
    pre_conv_buf: Vec<f32>,
    post_conv_overflow_buf: ringbuf::HeapRb<f32>,
}

impl SourceGroup {
    pub fn new(
        source_spec: AudioSpec,
        output_spec: AudioSpec,
        conversion_quality: Quality,
    ) -> Self {
        Self {
            spec: source_spec,
            sources: Vec::new(),
            channel_conv: make_channel_conversion(source_spec, output_spec),
            samplerate_conv: make_rate_conversion(source_spec, output_spec, conversion_quality),
            samplerate_conv_done_once: false,
            pre_conv_buf: vec![
                0.0f32;
                (source_spec.channels.get() as usize)
                    * (source_spec.samplerate.get() as usize)
            ],
            post_conv_overflow_buf: HeapRb::new(
                (source_spec.channels.get() as usize) * (source_spec.samplerate.get() as usize),
            ),
        }
    }

    pub fn sources_iter_mut(&mut self) -> impl Iterator<Item = &mut Source> {
        self.sources.iter_mut()
    }

    pub fn add_source(&mut self, source: Source) -> Result<(), MismatchedSpecError> {
        if self.spec == source.spec() {
            self.sources.push(source);
            Ok(())
        } else {
            Err(MismatchedSpecError)
        }
    }

    pub fn drop_completed_sources(&mut self) {
        self.sources
            .retain(|source| source.stream_state() != StreamState::Complete)
    }

    pub fn sources_len(&self) -> usize {
        self.sources.len()
    }

    pub fn mix_to_given_spec(&mut self, out_spec: AudioSpec, out_buffer: &mut [f32]) {
        if self.sources.is_empty() {
            return;
        }

        let self_chans = self.spec.channels.get() as usize;
        let out_chans = out_spec.channels.get() as usize;

        debug_assert!(out_buffer.len() % out_chans == 0);

        // TODO: implement Frames for HeapRb / SharedRb<Heap<f32>>
        debug_assert!(self.post_conv_overflow_buf.occupied_len() % out_chans == 0);

        let num_out_buffer_frames = out_buffer.len_frames(out_spec).get();

        let prior_overflow_frames_available =
            self.post_conv_overflow_buf.occupied_len() / out_chans;

        let prior_overflow_frames_drained =
            std::cmp::min(num_out_buffer_frames, prior_overflow_frames_available);

        out_buffer
            .iter_mut()
            .zip(self.post_conv_overflow_buf.pop_iter())
            .for_each(|(output, sample)| *output += sample);

        let out_spec_frames_needed = num_out_buffer_frames - prior_overflow_frames_drained;

        if out_spec_frames_needed == 0 {
            return;
        }

        let mut source_spec_frames_needed = (out_spec_frames_needed as f64)
            / ((out_spec.samplerate.get() as f64) / (self.spec.samplerate.get() as f64));

        // FIXME: hack for libsamplerate's "transport delay" which means the first call
        //        to .process() may return fewer frames than expected.
        if !self.samplerate_conv_done_once && self.spec != out_spec {
            source_spec_frames_needed *= 1.5;
            self.samplerate_conv_done_once = true;
        }

        let source_spec_frames_needed_ceil = source_spec_frames_needed.ceil() as usize;

        let mixbuf = self
            .pre_conv_buf
            .slice_frames_mut(self.spec, ..source_spec_frames_needed_ceil);

        mixbuf.fill(0.0f32);

        for source in self.sources.iter_mut() {
            source.mix_to_same_spec(mixbuf);
        }

        let mut mixbuf_iter: Box<dyn Iterator<Item = f32>> = Box::new(mixbuf.iter().copied());

        if let Some(ChannelConversion::DuplicateAndOrTruncate {
            input_channels,
            output_channels,
        }) = &self.channel_conv
        {
            let mut num_channels = input_channels.get();

            debug_assert_eq!(self_chans, num_channels as usize);

            while num_channels < output_channels.get() {
                mixbuf_iter = Box::new(mixbuf_iter.doubled());
                num_channels *= 2;
            }

            if num_channels > output_channels.get() {
                mixbuf_iter = Box::new(
                    mixbuf_iter
                        .drop_channels(num_channels as usize, output_channels.get() as usize),
                );
            }
        }

        if let Some(converter) = &self.samplerate_conv {
            let converted_samples = converter
                .process(mixbuf_iter.collect::<Vec<_>>().as_slice())
                .unwrap();

            debug_assert!(converted_samples.len() % out_chans == 0);

            let num_converted_frames = converted_samples.len_frames(out_spec).get();

            debug_assert!(num_converted_frames >= out_spec_frames_needed);

            let overflow_frames = num_converted_frames - out_spec_frames_needed;

            out_buffer
                .slice_frames_mut(out_spec, prior_overflow_frames_drained..)
                .iter_mut()
                .zip(converted_samples.iter())
                .for_each(|(output, sample)| *output += sample);

            self.post_conv_overflow_buf.push_slice(
                converted_samples
                    .slice_frames(out_spec, (num_converted_frames - overflow_frames)..),
            );
        } else {
            out_buffer
                .slice_frames_mut(out_spec, prior_overflow_frames_drained..)
                .iter_mut()
                .zip(mixbuf_iter)
                .for_each(|(output, sample)| *output += sample);
        }
    }
}

fn make_channel_conversion(
    input_spec: AudioSpec,
    output_spec: AudioSpec,
) -> Option<ChannelConversion> {
    match input_spec.channels.cmp(&output_spec.channels) {
        std::cmp::Ordering::Less | std::cmp::Ordering::Greater => {
            Some(ChannelConversion::DuplicateAndOrTruncate {
                input_channels: input_spec.channels,
                output_channels: output_spec.channels,
            })
        }
        std::cmp::Ordering::Equal => None,
    }
}

fn make_rate_conversion(
    input_spec: AudioSpec,
    output_spec: AudioSpec,
    quality: Quality,
) -> Option<SamplerateConverter> {
    match input_spec.samplerate.cmp(&output_spec.samplerate) {
        std::cmp::Ordering::Less | std::cmp::Ordering::Greater => Some(
            SamplerateConverter::new(
                match quality {
                    Quality::Lowest => ConverterType::Linear,
                    Quality::Low => ConverterType::SincFastest,
                    Quality::Medium => ConverterType::SincMediumQuality,
                    Quality::High => ConverterType::SincBestQuality,
                },
                input_spec.samplerate.get(),
                output_spec.samplerate.get(),
                output_spec.channels.get() as usize,
            )
            .unwrap(),
        ),
        std::cmp::Ordering::Equal => None,
    }
}
