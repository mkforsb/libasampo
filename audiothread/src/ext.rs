// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use std::ops::RangeBounds;

use crate::types::{AudioSpec, NumFrames};

/// Iterator adapter that zips an inner iterator with itself.
pub(crate) struct ZipSelf<I, T>
where
    I: Iterator<Item = T>,
    T: Copy,
{
    /// Inner iterator.
    inner: I,
}

impl<I, T> Iterator for ZipSelf<I, T>
where
    I: Iterator<Item = T>,
    T: Copy,
{
    type Item = (T, T);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|val| (val, val))
    }
}

/// Iterator operations for audio buffer iterators.
pub(crate) trait BufferIteratorOps<I, T> {
    /// Zip an iterator with itself.
    fn zip_self(self) -> ZipSelf<I, T>
    where
        I: Iterator<Item = T>,
        T: Copy;

    /// Double the number of channels in an interleaved audio stream.
    fn doubled(self) -> impl Iterator<Item = T>;

    /// Truncate the number of channels in an interleaved audio stream.
    ///
    /// # Arguments
    /// * `from` - Number of channels in original stream.
    /// * `to` - Number of channels in output stream.
    fn drop_channels(self, from: usize, to: usize) -> impl Iterator<Item = T>;
}

impl<I, T> BufferIteratorOps<I, T> for I
where
    I: Iterator<Item = T>,
    T: Copy,
{
    fn zip_self(self) -> ZipSelf<I, T>
    where
        I: Iterator<Item = T>,
        T: Copy,
    {
        ZipSelf { inner: self }
    }

    fn doubled(self) -> impl Iterator<Item = T> {
        self.zip_self()
            .flat_map(|(a, b)| std::iter::once(a).chain(std::iter::once(b)))
    }

    fn drop_channels(self, from: usize, to: usize) -> impl Iterator<Item = T> {
        assert!(from > to);
        self.enumerate()
            .filter(move |(idx, _)| idx % from < to)
            .map(|(_, val)| val)
    }
}

fn range_frames(
    spec: AudioSpec,
    range: impl RangeBounds<usize>,
) -> (std::ops::Bound<usize>, std::ops::Bound<usize>) {
    let chans = spec.channels.get() as usize;

    let from = match range.start_bound() {
        std::ops::Bound::Included(start) => std::ops::Bound::Included(chans * start),
        std::ops::Bound::Excluded(start) => std::ops::Bound::Excluded(chans * start),
        std::ops::Bound::Unbounded => std::ops::Bound::Unbounded,
    };

    let to = match range.end_bound() {
        std::ops::Bound::Included(end) => std::ops::Bound::Included(chans * end),
        std::ops::Bound::Excluded(end) => std::ops::Bound::Excluded(chans * end),
        std::ops::Bound::Unbounded => std::ops::Bound::Unbounded,
    };

    (from, to)
}

pub(crate) trait Frames {
    fn len_frames(&self, spec: AudioSpec) -> NumFrames;
    fn slice_frames(&self, spec: AudioSpec, range: impl RangeBounds<usize>) -> &[f32];
    fn slice_frames_mut(&mut self, spec: AudioSpec, range: impl RangeBounds<usize>) -> &mut [f32];
}

impl Frames for [f32] {
    fn len_frames(&self, spec: AudioSpec) -> NumFrames {
        debug_assert!(self.len() % (spec.channels.get() as usize) == 0);
        NumFrames::new(self.len() / (spec.channels.get() as usize))
    }

    fn slice_frames(&self, spec: AudioSpec, range: impl RangeBounds<usize>) -> &[f32] {
        debug_assert!(self.len() % (spec.channels.get() as usize) == 0);
        &self[range_frames(spec, range)]
    }

    fn slice_frames_mut(&mut self, spec: AudioSpec, range: impl RangeBounds<usize>) -> &mut [f32] {
        debug_assert!(self.len() % (spec.channels.get() as usize) == 0);
        &mut self[range_frames(spec, range)]
    }
}

impl Frames for Vec<f32> {
    fn len_frames(&self, spec: AudioSpec) -> NumFrames {
        debug_assert!(self.len() % (spec.channels.get() as usize) == 0);
        NumFrames::new(self.len() / (spec.channels.get() as usize))
    }

    fn slice_frames(&self, spec: AudioSpec, range: impl RangeBounds<usize>) -> &[f32] {
        debug_assert!(self.len() % (spec.channels.get() as usize) == 0);
        &self.as_slice()[range_frames(spec, range)]
    }

    fn slice_frames_mut(&mut self, spec: AudioSpec, range: impl RangeBounds<usize>) -> &mut [f32] {
        debug_assert!(self.len() % (spec.channels.get() as usize) == 0);
        &mut self.as_mut_slice()[range_frames(spec, range)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_iter_doubled() {
        let vals: Vec<f32> = vec![1.0, 2.0, 3.0];

        assert_eq!(
            vals.clone().into_iter().doubled().collect::<Vec<f32>>(),
            vec![1.0, 1.0, 2.0, 2.0, 3.0, 3.0]
        );
        assert_eq!(
            vals.into_iter().doubled().doubled().collect::<Vec<f32>>(),
            vec![1.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 2.0, 3.0, 3.0, 3.0, 3.0]
        );
    }

    #[test]
    fn test_buffer_iter_drop_chan() {
        let vals: Vec<f32> = vec![1.0, 2.0, 3.0, 1.0, 2.0, 3.0];

        assert_eq!(
            vals.clone()
                .into_iter()
                .drop_channels(3, 2)
                .collect::<Vec<f32>>(),
            vec![1.0, 2.0, 1.0, 2.0]
        );
        assert_eq!(
            vals.clone()
                .into_iter()
                .drop_channels(3, 1)
                .collect::<Vec<f32>>(),
            vec![1.0, 1.0]
        );
    }
}
