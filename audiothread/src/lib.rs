// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

#![deny(
    unused,
    missing_docs,
    clippy::missing_panics_doc,
    clippy::missing_safety_doc,
    clippy::missing_docs_in_private_items
)]

/*!
A simple way to spin up an audio stream over PulseAudio (hopefully others in the future)
that runs happily in its own thread and accepts simple messages for adding sound sources
into the mix (i.e playing sounds.)

The audio stream will be constantly running even if no sounds are playing, meaning there
should not be any kind of delay beyond basic audio server latency when starting playback
on a new sound source.

The audio thread handles converting sources to the correct sample rate and the correct
number of channels (i.e mono to stereo.)

Currently there is only one source supported ([SymphoniaSource]) which decodes files using
[Symphonia], and as such supports all the formats supported by Symphonia (e.g wav, mp3,
ogg and more.)

# Example
```no_run
use audiothread::{Opts, Message, SymphoniaSource};

let (tx, rx) = std::sync::mpsc::channel();
let _ = audiothread::spawn(rx, Some(Opts::default().with_name("Audio Thread")));
let sf = SymphoniaSource::from_file("/tmp/macarena.wav").unwrap();

tx.send(Message::PlaySymphoniaSource(sf));
```

[Symphonia]: https://docs.rs/symphonia/latest/symphonia/
*/

use std::cell::{BorrowMutError, Cell, RefCell, RefMut};
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::rc::Rc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use std::vec;

use libpulse_binding::context::{self, Context};
use libpulse_binding::def::{BufferAttr, Retval};
use libpulse_binding::mainloop::standard::Mainloop;
use libpulse_binding::sample::{Format, Spec};
use libpulse_binding::stream::{self, SeekMode, Stream};
use samplerate::{ConverterType, Samplerate};
use symphonia::core::io::MediaSource;
use symphonia::core::{
    audio::SampleBuffer, codecs::Decoder, formats::FormatReader, io::MediaSourceStream, probe::Hint,
};
use thiserror::Error as ThisError;

/// Audio output frame size in bytes.
///
/// A frame contains 1 sample point (in some format, e.g f32) * N channels.
const FRAMESIZE: usize = 8;

thread_local! {
    /// Libsamplerate converter type (quality setting, essentially)
    static RATE_CONV_TYPE: Cell<ConverterType> = const { Cell::new(ConverterType::Linear) };

    /// Audio output specification.
    static SPEC: Cell<Spec> = const { Cell::new(Spec {
        format: Format::FLOAT32NE,
        rate: 48000,
        channels: 2,
    })};
}

/// A trait for lazy iterators that can consume their elements without forming a
/// collection. Useful for iterators that produce side-effects.
trait Consume<I>
where
    I: Iterator,
{
    /// Consume and discard all elements.
    fn consume(self);
}

impl<T> Consume<T> for T
where
    T: Iterator,
{
    fn consume(self) {
        for _x in self {}
    }
}

/// Iterator adapter that zips an inner iterator with itself.
struct ZipSelf<I, T>
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
trait BufferIteratorOps<I, T> {
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

/// Error types of [`SymphoniaSource`] operations.
#[derive(Debug, ThisError)]
pub enum SymphoniaSourceError {
    /// General IO error.
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Symphonia library error.
    #[error("Symphonia error: {0}")]
    SymphoniaError(#[from] symphonia::core::errors::Error),

    /// File/format did not provide a default track.
    #[error("No default track")]
    NoDefaultTrackError(),
}

/// Represents an audio buffer being decoded by Symphonia.
pub struct SymphoniaSource {
    /// Symphonia [`FormatReader`]
    reader: Box<dyn FormatReader>,

    /// Symphonia [`Decoder`]
    decoder: Box<dyn Decoder>,

    /// Track id, as per [`symphonia::core::formats::FormatReader::default_track`]
    track_id: u32,

    /// Current output buffer (decoded and converted to audio spec of the audiothread)
    cur_buffer: Option<vec::IntoIter<f32>>,

    /// Indicates end-of-file/stream.
    eof: bool,
}

impl std::fmt::Debug for SymphoniaSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!(
            "SymphoniaSource(codec: {:?}, track_id: {}, buffer: {}, eof: {})",
            self.decoder.codec_params(),
            self.track_id,
            match &self.cur_buffer {
                Some(iter) => format!("Some({})", {
                    let (lower, upper) = iter.size_hint();
                    format!("items lower bound: {lower:?}, upper bound: {upper:?}")
                }),
                None => String::from("None"),
            },
            self.eof,
        ))
    }
}

impl Iterator for SymphoniaSource {
    // TODO: optimizations:
    // - an Iterator that branches on every item is unfortunate
    // - eliminate or reduce unnecessary .to_vec/.collect allocs

    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        match match self.cur_buffer {
            Some(ref mut buf) => match buf.next() {
                Some(val) => Some(val),
                None => {
                    self.cur_buffer = None;
                    self.next()
                }
            },
            None => {
                let mut packet = self.reader.next_packet().ok()?;

                while packet.track_id() != self.track_id {
                    packet = self.reader.next_packet().ok()?;
                }

                match self.decoder.decode(&packet) {
                    Ok(audiobuf) => {
                        let spec = SPEC.get();
                        let buf_rate = audiobuf.spec().rate;
                        let mut chans = audiobuf.spec().channels.count();

                        let mut samplebuf =
                            SampleBuffer::<f32>::new(audiobuf.capacity() as u64, *audiobuf.spec());
                        samplebuf.copy_interleaved_ref(audiobuf);

                        let mut resultbuf = samplebuf.samples().to_vec();

                        while chans < spec.channels.into() {
                            resultbuf = resultbuf.into_iter().doubled().collect();
                            chans *= 2;
                        }

                        if chans > spec.channels.into() {
                            resultbuf = resultbuf
                                .into_iter()
                                .drop_channels(chans, spec.channels.into())
                                .collect();
                        }

                        if buf_rate != spec.rate {
                            let converter_type =
                                RATE_CONV_TYPE.with(|val| val.clone().into_inner());

                            let conv = Samplerate::new(
                                converter_type,
                                buf_rate,
                                spec.rate,
                                spec.channels as usize,
                            )
                            .unwrap();

                            resultbuf = conv.process_last(resultbuf.as_slice()).unwrap()
                        }

                        self.cur_buffer = Some(resultbuf.into_iter());
                        self.next()
                    }
                    Err(_) => None,
                }
            }
        } {
            Some(val) => Some(val),
            None => {
                self.eof = true;
                None
            }
        }
    }
}

/// A wrapper for a BufReader with an extra field to cache the length of the buffer,
/// suitable for an implementation of [`symphonia::core::io::MediaSource`].
struct BufReadWrap<R: Read + Seek + Send + Sync> {
    /// Inner BufReader.
    bufreader: BufReader<R>,

    /// Length of buffer.
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

impl<R: Read + Seek + Send + Sync> MediaSource for BufReadWrap<R> {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        self.len
    }
}

impl SymphoniaSource {
    /// Open an audio file for decoding with Symphonia from a given path.
    ///
    /// # Arguments
    /// * `path` - Path to file.
    ///
    /// # Examples
    /// ```no_run
    /// use audiothread::{Opts, Message, SymphoniaSource};
    ///
    /// let (tx, rx) = std::sync::mpsc::channel();
    /// let _ = audiothread::spawn(rx, Some(Opts::default().with_name("Audio Thread")));
    /// let src = SymphoniaSource::from_file("/tmp/macarena.wav").unwrap();
    ///
    /// tx.send(Message::PlaySymphoniaSource(src));
    /// ```
    pub fn from_file(path: &str) -> Result<SymphoniaSource, SymphoniaSourceError> {
        Self::from_buf_reader(BufReader::new(File::open(path)?))
    }

    /// Create a Symphonia audio source from an arbitrary buffer.
    ///
    /// # Arguments
    /// * `bufreader` - A BufReader over some buffer.
    ///
    /// # Examples
    /// ```no_run
    /// use audiothread::{Opts, Message, SymphoniaSource};
    ///
    /// let (tx, rx) = std::sync::mpsc::channel();
    /// let _ = audiothread::spawn(rx, Some(Opts::default().with_name("Audio Thread")));
    /// let buffer: Vec<u8> = vec![0x52, 0x49, 0x46, 0x46];
    /// let src = SymphoniaSource::from_buf_reader(
    ///     std::io::BufReader::new(std::io::Cursor::new(buffer))
    /// ).unwrap();
    ///
    /// tx.send(Message::PlaySymphoniaSource(src));
    /// ```
    pub fn from_buf_reader<R: Read + Seek + Send + Sync + 'static>(
        mut bufreader: BufReader<R>,
    ) -> Result<SymphoniaSource, SymphoniaSourceError> {
        let len = bufreader.seek(std::io::SeekFrom::End(0)).ok();
        let _ = bufreader.seek(std::io::SeekFrom::Start(0));

        let mss =
            MediaSourceStream::new(Box::new(BufReadWrap { bufreader, len }), Default::default());

        match symphonia::default::get_probe().format(
            &Hint::new(),
            mss,
            &Default::default(),
            &Default::default(),
        ) {
            Ok(probed) => {
                let codecs = symphonia::default::get_codecs();
                let track_id: u32 = probed
                    .format
                    .default_track()
                    .ok_or(SymphoniaSourceError::NoDefaultTrackError())?
                    .id;
                let codec_params = &probed
                    .format
                    .default_track()
                    .ok_or(SymphoniaSourceError::NoDefaultTrackError())?
                    .codec_params;
                let decoder = codecs.make(codec_params, &Default::default())?;

                Ok(SymphoniaSource {
                    reader: probed.format,
                    decoder,
                    track_id,
                    cur_buffer: None,
                    eof: false,
                })
            }
            Err(e) => Err(e.into()),
        }
    }
}

/// Convenience function to extract the contents of a borrowed [`RefCell<Option>`]
fn grab<T>(r: Result<RefMut<Option<T>>, BorrowMutError>) -> Option<RefMut<T>> {
    r.map(|x| RefMut::map(x, |y| y.as_mut().unwrap())).ok()
}

/// State of an audio source stream.
enum StreamState {
    /// Actively streaming data.
    Streaming,

    /// Reached end of stream.
    Complete,
}

/// Audio sources.
enum Source {
    /// Audio source decoded by Symphonia.
    SymphoniaSource(StreamState, SymphoniaSource),

    /// A null audio source.
    None,
}

impl Source {
    /// Set the state of an audio source stream.
    ///
    /// # Arguments
    /// * `state` - New state.
    fn set_state(&mut self, state: StreamState) {
        *self = match std::mem::replace(self, Source::None) {
            Source::SymphoniaSource(_, sf) => Source::SymphoniaSource(state, sf),
            Source::None => panic!("cannot set state on this Source variant"),
        }
    }
}

/// Mix an audio source into a given buffer.
///
/// # Arguments
/// * `source` - Audio source.
/// * `output` - Target buffer.
fn mix(source: &mut Source, output: &mut [f32]) {
    match source {
        Source::SymphoniaSource(StreamState::Streaming, ref mut sf) => {
            if sf.eof {
                source.set_state(StreamState::Complete);
            } else {
                output
                    .iter_mut()
                    .zip(sf)
                    .map(|(out, v)| {
                        *out += v;
                    })
                    .consume();
            }
        }

        Source::SymphoniaSource(StreamState::Complete, _) => (),

        Source::None => (),
    }
}

/// Spawn a new audio thread.
///
/// # Examples
///
/// ```no_run
/// use audiothread::{Opts, Message, SymphoniaSource, Quality};
///
/// let (tx, rx) = std::sync::mpsc::channel();
/// let _ = audiothread::spawn(rx, Some(Opts::default()
///                                          .with_name("Music")
///                                          .with_sr_conv_quality(Quality::High)));
///
/// tx.send(Message::PlaySymphoniaSource(SymphoniaSource::from_file("/tmp/mozart.ogg")
///                                                      .unwrap()));
/// ```
///
/// # Arguments
/// * `rx` - Receiving end of channel for [`Message`]s.
/// * `opts` - Optional custom options via [`Opts`].
///
/// [converter type]: https://docs.rs/samplerate/latest/samplerate/converter_type/enum.ConverterType.html
pub fn spawn(rx: mpsc::Receiver<Message>, opts: Option<Opts>) -> JoinHandle<()> {
    thread::spawn(move || threadloop(rx, opts))
}

/// Errors that may arise in MPSC-channel operations.
#[derive(Debug)]
enum ChannelError {
    /// The channel is disconnected.
    Disconnected,
}

/// Fetch all messages on a receiver using a timeout for the first one but not for any
/// subsequent ones, thus the call should sleep at most as long as the given duration.
fn recv_all(rx: &std::sync::mpsc::Receiver<Message>, timeout: Duration) -> Result<Option<Vec<Message>>, ChannelError> {
    match rx.recv_timeout(timeout) {
        Ok(message) => {
            let mut messages = vec![message];

            let mut quit = false;
            let mut disconnected = false;

            while !quit {
                match rx.try_recv() {
                    Ok(message) => { messages.push(message) },
                    Err(mpsc::TryRecvError::Empty) => { quit = true; },
                    Err(mpsc::TryRecvError::Disconnected) => {
                        quit = true;
                        disconnected = true;
                    },
                }
            }

            if disconnected {
                Err(ChannelError::Disconnected)
            } else {
                Ok(Some(messages))
            }
        },

        Err(err) => match err {
            RecvTimeoutError::Timeout => Ok(None),
            RecvTimeoutError::Disconnected => Err(ChannelError::Disconnected),
        }
    }
}

/// The main loop of an audiothread.
///
/// # Panics
///
/// This function will panic if the audio spec is invalid or has the wrong frame size.
fn threadloop(rx: mpsc::Receiver<Message>, opts: Option<Opts>) {
    let opts = opts.unwrap_or_default();

    RATE_CONV_TYPE.replace(match opts.sr_conv_quality {
        Quality::Fastest => ConverterType::ZeroOrderHold,
        Quality::Low => ConverterType::Linear,
        Quality::Medium => ConverterType::SincMediumQuality,
        Quality::High => ConverterType::SincBestQuality,
    });

    SPEC.replace(Spec {
        rate: opts.sample_rate,
        ..SPEC.get()
    });

    assert!(SPEC.get().is_valid());
    assert!(SPEC.get().frame_size() == FRAMESIZE);

    log::log!(log::Level::Info, "Audiothread starting up");

    let mut pa_mainloop = Mainloop::new().expect("Libpulse can allocate a mainloop");

    let pa_context = Rc::new(RefCell::new(
        Context::new(&pa_mainloop, &opts.stream_name).expect("Libpulse can allocate a context"),
    ));
    let pa_context_csc = Rc::clone(&pa_context);

    let pa_stream: Rc<RefCell<Option<Stream>>> = Rc::new(RefCell::new(None));
    let pa_stream_csc = Rc::clone(&pa_stream);
    let pa_stream_ssc = Rc::clone(&pa_stream);
    let pa_stream_srw = Rc::clone(&pa_stream);

    let sources: Rc<RefCell<Vec<Source>>> = Rc::new(RefCell::new(Vec::new()));
    let sources_srw = Rc::clone(&sources);

    let stream_ready_write = move |n: usize| {
        log::log!(
            log::Level::Debug,
            "Stream ready for writing, accepting {} frames",
            n / FRAMESIZE,
        );

        match grab(pa_stream_srw.try_borrow_mut()) {
            Some(ref mut s) => match s.begin_write(Some(n)) {
                Ok(Some(ref mut buf)) => {
                    let frames_to_write = buf.len() / FRAMESIZE;

                    log::log!(
                        log::Level::Debug,
                        "Provided buffer capacity is {frames_to_write} frames"
                    );

                    // TODO: skip if no sources are playing.
                    // complication: the buffer received from .begin_write may need to
                    // to be zeroed once after the last playing source is dropped.
                    unsafe {
                        let (_, buf_f32, _) = buf.align_to_mut::<f32>();
                        buf_f32.fill(0.0);

                        for src in sources_srw.borrow_mut().iter_mut() {
                            mix(src, buf_f32);
                        }
                    }

                    if let Err(e) = s.write(buf, None, 0, SeekMode::Relative) {
                        log::log!(log::Level::Warn, "Error writing to stream: {:?}", e);
                    }
                }
                Ok(None) => log::log!(
                    log::Level::Error,
                    "Stream ready for writing, but .begin_write failed to provide a buffer"
                ),
                Err(e) => log::log!(
                    log::Level::Error,
                    "Stream ready for writing, but .begin_write failed with error {:?}",
                    e
                ),
            },

            // State/logic bug, if it ever happens
            None => panic!("Stream ready for writing, but not able to be borrowed or not present"),
        }
    };

    let stream_state_changed = move || {
        #[allow(clippy::single_match)]
        match pa_stream_ssc.try_borrow() {
            Ok(ref stream_opt) => match stream_opt.as_ref() {
                Some(stream) => log::log!(
                    log::Level::Debug,
                    "Stream state changed: {:?}",
                    stream.get_state()
                ),
                None => (),
            },
            Err(_) => (),
        }
    };

    let context_state_changed = move || match pa_context_csc.try_borrow_mut() {
        Ok(ref mut ctx) => {
            log::log!(
                log::Level::Debug,
                "Context state changed: {:?}",
                ctx.get_state()
            );

            if pa_stream_csc.try_borrow_mut().is_ok() {
                let have_stream = pa_stream_csc.borrow_mut().is_some();

                if !have_stream {
                    let mut s = Stream::new(ctx, "stream", &SPEC.get(), None)
                        .expect("Libpulse can allocate a stream");

                    s.set_state_callback(Some(Box::new(stream_state_changed.clone())));
                    s.set_write_callback(Some(Box::new(stream_ready_write.clone())));

                    pa_stream_csc.replace(Some(s));

                    log::log!(log::Level::Info, "Stream created");
                }
            }
        }
        Err(_) => log::log!(log::Level::Debug, "Context state changed: {{n/a}}"),
    };

    pa_context
        .borrow_mut()
        .set_state_callback(Some(Box::new(context_state_changed.clone())));

    pa_context
        .borrow_mut()
        .connect(None, context::FlagSet::NOAUTOSPAWN, None)
        .expect("Created connection to PulseAudio");

    log::log!(
        log::Level::Info,
        "Connected to server {:?}",
        pa_context.borrow().get_server()
    );

    let mut stream_asked_connect = false;
    let mut stream_created = false;

    let mut since_cleanup = Instant::now();

    loop {
        pa_mainloop.iterate(false);

        if !stream_created {
            stream_created = pa_stream.borrow().is_some();
        }

        if stream_created && !stream_asked_connect {
            #[allow(clippy::single_match)]
            match pa_stream.borrow_mut().as_mut().unwrap().connect_playback(
                None,
                // None,
                Some(&BufferAttr {
                    maxlength: (opts.bufsize_n_stereo_samples * FRAMESIZE) as u32,
                    tlength: (opts.bufsize_n_stereo_samples * FRAMESIZE) as u32,
                    prebuf: 0,
                    minreq: (opts.bufsize_n_stereo_samples * FRAMESIZE) as u32,
                    fragsize: 0,
                }),
                stream::FlagSet::ADJUST_LATENCY,
                None,
                None,
            ) {
                Ok(_) => stream_asked_connect = true,
                _ => (),
            }
        }

        let mut quit = false;

        // FIXME: what if sleeping here causes underflow on the sound server?
        // simple way to cause the issue is to play a very large number of sources simultaneously
        // maybe the timeout value here should be a parameter, or adjusted based on load
        match recv_all(&rx, Duration::from_millis(2)) {
            Ok(Some(messages)) => {
                for message in messages {
                    match message {
                        Message::Shutdown() => {
                            quit = true;
                            break;
                        }
                        Message::DropAll() => sources.borrow_mut().clear(),
                        Message::PlaySymphoniaSource(sf) => sources
                            .borrow_mut()
                            .push(Source::SymphoniaSource(StreamState::Streaming, sf)),
                    }
                }
            },

            Ok(None) => (),
            Err(ChannelError::Disconnected) => {
                log::log!(log::Level::Error, "Message channel disconnected, shutting down");
                break
            },
        }

        if quit {
            break;
        }

        if since_cleanup.elapsed().as_millis() >= 1000 {
            since_cleanup = Instant::now();

            sources
                .borrow_mut()
                .retain(|src| matches!(src, Source::SymphoniaSource(StreamState::Streaming, _)));

            log::log!(
                log::Level::Debug,
                "{} sources playing",
                sources.borrow().len()
            );
        }
    }

    log::log!(log::Level::Info, "Audiothread shutting down gracefully");

    if stream_asked_connect {
        pa_stream
            .borrow_mut()
            .as_mut()
            .unwrap()
            .disconnect()
            .expect("We should have disconnected from PulseAudio");
    }

    pa_context.borrow_mut().disconnect();

    // is this needed/beneficial?
    pa_mainloop.quit(Retval(0));
}

/// Messages available to send to a running audiothead.
#[derive(Debug)]
pub enum Message {
    /// Stop the thread.
    Shutdown(),

    /// Stop and discard all sounds.
    DropAll(),

    /// Play an audio file being decoded by Symphonia.
    PlaySymphoniaSource(SymphoniaSource),
}

/// Quality levels.
#[derive(Debug, Clone, PartialEq)]
pub enum Quality {
    /// Use whatever is the fastest setting.
    Fastest,

    /// Use low quality.
    Low,

    /// Use medium quality.
    Medium,

    /// Use high quality.
    High,
}

/// Audio thread options.
///
/// # Default values
/// ```
/// let opts = audiothread::Opts::default();
///
/// assert_eq!(opts.stream_name, "Audio");
/// assert_eq!(opts.sample_rate, 48000);
/// assert_eq!(opts.sr_conv_quality, audiothread::Quality::Medium);
/// assert_eq!(opts.bufsize_n_stereo_samples, 2048);
/// ```
///
/// # Example
///
/// ```no_run
/// let opts = audiothread::Opts::default()
///     .with_name("Music Stream")
///     .with_sample_rate(44100)
///     .with_sr_conv_quality(audiothread::Quality::Fastest)
///     .with_bufsize_n_stereo_samples(1024);
/// ```
#[derive(Debug)]
pub struct Opts {
    /// Name to register with sound server.
    pub stream_name: String,

    /// Output sample rate.
    pub sample_rate: u32,

    /// Sample rate conversion quality.
    pub sr_conv_quality: Quality,

    /// Buffer size in number of stereo samples.
    pub bufsize_n_stereo_samples: usize,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            stream_name: String::from("Audio"),
            sample_rate: 48000,
            sr_conv_quality: Quality::Medium,
            bufsize_n_stereo_samples: 2048,
        }
    }
}

impl Opts {
    /// Create a new set of options.
    pub fn new<T: Into<String>>(
        name: T,
        sample_rate: u32,
        conv_quality: Quality,
        bufsize_n_stereo_samples: usize,
    ) -> Self {
        Opts {
            stream_name: name.into(),
            sample_rate,
            sr_conv_quality: conv_quality,
            bufsize_n_stereo_samples,
        }
    }

    /// Make an updated option set using the given name.
    pub fn with_name<T: Into<String>>(self, name: T) -> Self {
        Opts {
            stream_name: name.into(),
            ..self
        }
    }

    /// Make an updated option set using the given output sample rate.
    pub fn with_sample_rate(self, sample_rate: u32) -> Self {
        Opts {
            sample_rate,
            ..self
        }
    }

    /// Make an updated option set using the given sample rate conversion quality.
    pub fn with_sr_conv_quality(self, conv_quality: Quality) -> Self {
        Opts {
            sr_conv_quality: conv_quality,
            ..self
        }
    }

    /// Make an updated option set using the given buffer size of N stereo samples.
    pub fn with_bufsize_n_stereo_samples(self, bufsize_n_stereo_samples: usize) -> Self {
        Opts {
            bufsize_n_stereo_samples,
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_spec() {
        assert!(SPEC.get().is_valid());
        assert_eq!(SPEC.get().frame_size(), FRAMESIZE);
    }

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

    #[test]
    fn test_recv_all() {
        fn fmt_message(x: Message) -> String {
            match x {
                Message::Shutdown() => String::from("Shutdown"),
                Message::DropAll() => String::from("DropAll"),
                Message::PlaySymphoniaSource(..) => String::from("PlaySymphoniaSource"),
            }
        }

        let (tx, rx) = mpsc::channel::<Message>();

        assert!(recv_all(&rx, Duration::from_millis(0)).is_ok_and(|m| m.is_none()));

        tx.send(Message::DropAll()).unwrap();
        tx.send(Message::Shutdown()).unwrap();
        tx.send(Message::DropAll()).unwrap();

        let x = recv_all(&rx, Duration::from_millis(0))
            .unwrap()
            .unwrap()
            .into_iter()
            .map(fmt_message)
            .collect::<Vec<String>>();

        assert_eq!(x, vec!["DropAll", "Shutdown", "DropAll"]);
        assert!(recv_all(&rx, Duration::from_millis(0)).is_ok_and(|m| m.is_none()));
    }

    #[test]
    fn test_symphoniafile_mono_to_stereo() {
        let sf = SymphoniaSource::from_file(&format!(
            "{}/test_assets/square_1ch_48k_20smp.wav",
            env::var("CARGO_MANIFEST_DIR").unwrap()
        ))
        .unwrap();

        assert_eq!(sf.count(), 40);
    }

    #[test]
    fn test_symphoniafile_44k_to_48k() {
        let sf = SymphoniaSource::from_file(&format!(
            "{}/test_assets/silence_2ch_44.1k_88200smp.wav",
            env::var("CARGO_MANIFEST_DIR").unwrap()
        ))
        .unwrap();

        // The sample count won't be exact due to rounding errors, made worse by
        // converting over many small chunks rather than the whole thing at once?
        assert!((((sf.count() as f64) / 96000.0) - 1.0).abs() < 0.001); // under 0.1% error
    }

    #[test]
    fn test_symphoniafile_iter_eof() {
        let mut sf = SymphoniaSource::from_file(&format!(
            "{}/test_assets/square_1ch_48k_20smp.wav",
            env::var("CARGO_MANIFEST_DIR").unwrap()
        ))
        .unwrap();

        while let Some(_) = sf.next() {}

        assert_eq!(sf.eof, true);
    }

    #[test]
    fn test_set_stream_state() {
        let sf = SymphoniaSource::from_file(&format!(
            "{}/test_assets/square_1ch_48k_20smp.wav",
            env::var("CARGO_MANIFEST_DIR").unwrap()
        ))
        .unwrap();

        let mut src = Source::SymphoniaSource(StreamState::Streaming, sf);
        src.set_state(StreamState::Complete);

        assert!(
            if let Source::SymphoniaSource(StreamState::Complete, _) = src {
                true
            } else {
                false
            }
        );
    }

    #[test]
    fn test_mix() {
        let sf1 = SymphoniaSource::from_file(&format!(
            "{}/test_assets/square_1ch_48k_20smp.wav",
            env::var("CARGO_MANIFEST_DIR").unwrap()
        ))
        .unwrap();

        let sf2 = SymphoniaSource::from_file(&format!(
            "{}/test_assets/square_1ch_48k_20smp.wav",
            env::var("CARGO_MANIFEST_DIR").unwrap()
        ))
        .unwrap();

        let mut src1 = Source::SymphoniaSource(StreamState::Complete, sf1);
        let mut src2 = Source::SymphoniaSource(StreamState::Streaming, sf2);

        let mut buffer = [0.0; 40];

        mix(&mut src1, &mut buffer);
        assert_eq!(true, buffer.iter().all(|x| *x == 0.0));

        mix(&mut src2, &mut buffer);
        assert_eq!(false, buffer.iter().all(|x| *x == 0.0));
    }

    #[test]
    fn test_mix_symphoniafile_complete() {
        let sf = SymphoniaSource::from_file(&format!(
            "{}/test_assets/square_1ch_48k_20smp.wav",
            env::var("CARGO_MANIFEST_DIR").unwrap()
        ))
        .unwrap();

        let mut src = Source::SymphoniaSource(StreamState::Streaming, sf);
        let mut buffer = [0.0; 60];

        mix(&mut src, &mut buffer); // exhausts the iterator, causing sf.eof -> true
        mix(&mut src, &mut buffer); // detects .eof, updates stream state

        assert!(
            if let Source::SymphoniaSource(StreamState::Complete, _) = src {
                true
            } else {
                false
            }
        );
    }

    #[test]
    fn test_opts() {
        let opts = Opts::default();

        let opts = opts.with_name("Sound Effects");
        let opts = opts.with_sample_rate(22500);
        let opts = opts.with_sr_conv_quality(Quality::Medium);
        let opts = opts.with_bufsize_n_stereo_samples(31415);

        assert_eq!(opts.stream_name, "Sound Effects");
        assert_eq!(opts.sample_rate, 22500);
        assert_eq!(opts.sr_conv_quality, Quality::Medium);
        assert_eq!(opts.bufsize_n_stereo_samples, 31415);

        let opts = Opts::default()
            .with_name("Background Music")
            .with_sr_conv_quality(Quality::High);

        assert_eq!(opts.stream_name, "Background Music");
        assert_eq!(opts.sr_conv_quality, Quality::High);
    }

    #[ignore]
    #[test]
    fn test_threadloop() {
        // TODO: needs libpulse mocks
    }

    #[ignore]
    #[test]
    fn test_opts_applied() {
        // TODO: test opts being applied by spawn/threadloop
    }
}
