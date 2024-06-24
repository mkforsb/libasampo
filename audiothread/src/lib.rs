// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::mpsc::{self, RecvTimeoutError, Sender},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use libpulse_binding::{
    context::{Context as PulseContext, FlagSet as PulseContextFlagSet},
    def::{BufferAttr as PulseBufferAttr, Retval as PulseRetval},
    mainloop::standard::Mainloop as PulseMainloop,
    sample::{Format as PulseSampleFormat, Spec as PulseSampleSpec},
    stream::{FlagSet as PulseStreamFlagSet, SeekMode, Stream as PulseStream},
};

mod error;
mod ext;
mod source;
mod types;

use crate::{
    error::ChannelDisconnectedError,
    source::{pulled::PulledSource, Source, SourceGroup, SourceOps},
};

pub use crate::{
    source::{
        pulled::{PulledSourcePullReply, PulledSourcePullRequest, PulledSourceSetup},
        symphonia::SymphoniaSource,
    },
    types::{AudioSpec, NonZeroNumFrames, NumChannels, NumFrames, Quality, Samplerate},
};

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Message {
    Shutdown,
    DropAll,
    PlaySymphoniaSource(SymphoniaSource),
    CreatePulledSource(PulledSourceSetup),
    GetOutputSpec(Sender<AudioSpec>),
}

#[derive(Debug)]
pub struct Opts {
    stream_name: String,
    spec: AudioSpec,
    conversion_quality: Quality,
    buffer_size: NonZeroNumFrames,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            stream_name: "Audio".to_string(),
            spec: AudioSpec::new(48000, 2).unwrap(),
            conversion_quality: Quality::Medium,
            buffer_size: 2048.try_into().unwrap(),
        }
    }
}

impl Opts {
    pub fn new<T: Into<String>>(
        name: T,
        spec: AudioSpec,
        conversion_quality: Quality,
        buffer_size: NonZeroNumFrames,
    ) -> Self {
        Opts {
            stream_name: name.into(),
            spec,
            conversion_quality,
            buffer_size,
        }
    }

    pub fn with_name<T: Into<String>>(self, name: T) -> Self {
        Opts {
            stream_name: name.into(),
            ..self
        }
    }

    pub fn with_spec(self, spec: AudioSpec) -> Self {
        Opts { spec, ..self }
    }

    pub fn with_conversion_quality(self, conversion_quality: Quality) -> Self {
        Opts {
            conversion_quality,
            ..self
        }
    }

    pub fn with_buffer_size(self, buffer_size: NonZeroNumFrames) -> Self {
        Opts {
            buffer_size,
            ..self
        }
    }
}

fn recv_all(
    rx: &std::sync::mpsc::Receiver<Message>,
    timeout: Duration,
) -> Result<Option<Vec<Message>>, ChannelDisconnectedError> {
    match rx.recv_timeout(timeout) {
        Ok(message) => {
            let mut messages = vec![message];

            let mut quit = false;

            while !quit {
                match rx.try_recv() {
                    Ok(message) => messages.push(message),
                    Err(_) => {
                        quit = true;
                    }
                }
            }

            Ok(Some(messages))
        }

        Err(err) => match err {
            RecvTimeoutError::Timeout => Ok(None),
            RecvTimeoutError::Disconnected => Err(ChannelDisconnectedError),
        },
    }
}

pub fn spawn(rx: mpsc::Receiver<Message>, opts: Option<Opts>) -> JoinHandle<()> {
    thread::spawn(move || threadloop(rx, opts))
}

fn threadloop(rx: mpsc::Receiver<Message>, opts: Option<Opts>) {
    let opts = opts.unwrap_or_default();

    let conversion_quality = opts.conversion_quality;
    let output_spec = opts.spec;
    let framesize_bytes: usize = 4 * output_spec.channels.get() as usize;

    let pulse_spec = PulseSampleSpec {
        format: PulseSampleFormat::FLOAT32NE,
        rate: output_spec.samplerate.get(),
        channels: output_spec.channels.get(),
    };

    assert!(pulse_spec.is_valid());

    log::log!(log::Level::Info, "Audiothread starting up ({pulse_spec:?})");

    let mut pa_mainloop =
        PulseMainloop::new().expect("Libpulse should be able to allocate a mainloop");

    let mut pa_context = PulseContext::new(&pa_mainloop, &opts.stream_name)
        .expect("Libpulse should be able to alloate a context");

    let pa_context_raw: *mut PulseContext = &mut pa_context;
    let pa_context_csc = pa_context_raw;

    let sourcegroups: Rc<RefCell<HashMap<AudioSpec, SourceGroup>>> =
        Rc::new(RefCell::new(HashMap::new()));

    let sourcegroups_srw = Rc::clone(&sourcegroups);

    let context_state_changed = move || {
        log::log!(log::Level::Debug, "Context state changed: {:?}", unsafe {
            (*pa_context_csc).get_state()
        })
    };

    pa_context.set_state_callback(Some(Box::new(context_state_changed)));

    pa_context
        .connect(None, PulseContextFlagSet::NOAUTOSPAWN, None)
        .expect("We should be able to connect to PulseAudio");

    log::log!(
        log::Level::Info,
        "Connected to server {:?}",
        pa_context.get_server()
    );

    let context_ready_timer = std::time::Instant::now();
    let context_ready_timeout = std::time::Duration::from_secs(5);

    while pa_context.get_state() != libpulse_binding::context::State::Ready {
        match pa_mainloop.iterate(true) {
            libpulse_binding::mainloop::standard::IterateResult::Success(_) => (),
            libpulse_binding::mainloop::standard::IterateResult::Quit(_) => {
                panic!("PulseAudio quit while audiothread was connecting")
            }
            libpulse_binding::mainloop::standard::IterateResult::Err(e) => {
                panic!("PulseAudio error while audiothread was connecting: {e}")
            }
        }

        if context_ready_timer.elapsed() > context_ready_timeout {
            panic!("Timed out waiting for context to become ready");
        }
    }

    let mut stream =
        PulseStream::new(&mut pa_context, "My Stream", &pulse_spec, None).expect("stream");

    let stream_raw: *mut PulseStream = &mut stream;
    let stream_ssc = stream_raw;
    let stream_srw = stream_raw;

    let stream_state_changed = move || {
        log::log!(log::Level::Debug, "Stream state changed: {:?}", unsafe {
            (*stream_ssc).get_state()
        },);
    };

    stream.set_state_callback(Some(Box::new(stream_state_changed)));

    let stream_ready_write = move |n: usize| {
        debug_assert!(n % framesize_bytes == 0);

        // TODO: skip if no sources are playing.
        //       complication: the buffer received from .begin_write may need to
        //       to be zeroed once after the last playing source is dropped.
        unsafe {
            match (*stream_srw).begin_write(Some(n)) {
                Ok(Some(ref mut buf)) => {
                    let (prefix, buf_f32, suffix) = buf.align_to_mut::<f32>();

                    debug_assert!(prefix.is_empty());
                    debug_assert!(suffix.is_empty());

                    buf_f32.fill(0.0);

                    for (spec, group) in sourcegroups_srw.borrow_mut().iter_mut() {
                        if *spec == output_spec {
                            for source in group.sources_iter_mut() {
                                source.mix_to_same_spec(buf_f32);
                            }
                        } else {
                            group.mix_to_given_spec(output_spec, buf_f32);
                        }
                    }

                    if let Err(e) = (*stream_srw).write(buf, None, 0, SeekMode::Relative) {
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
            }
        }
    };

    stream.set_write_callback(Some(Box::new(stream_ready_write)));

    stream
        .connect_playback(
            None,
            Some(&PulseBufferAttr {
                maxlength: (opts.buffer_size.get() * framesize_bytes) as u32,
                tlength: (opts.buffer_size.get() * framesize_bytes) as u32,
                prebuf: 0,
                minreq: (opts.buffer_size.get() * framesize_bytes) as u32,
                fragsize: 0,
            }),
            PulseStreamFlagSet::ADJUST_LATENCY,
            None,
            None,
        )
        .expect("PulseAudio should let us connect our stream for playback");

    let stream_ready_timer = std::time::Instant::now();
    let stream_ready_timeout = std::time::Duration::from_secs(5);

    while stream.get_state() != libpulse_binding::stream::State::Ready {
        match pa_mainloop.iterate(true) {
            libpulse_binding::mainloop::standard::IterateResult::Success(_) => (),
            libpulse_binding::mainloop::standard::IterateResult::Quit(_) => {
                panic!("PulseAudio quit while audiothread was creating a stream")
            }
            libpulse_binding::mainloop::standard::IterateResult::Err(e) => {
                panic!("PulseAudio error while audiothread was creating a stream: {e}")
            }
        }

        if stream_ready_timer.elapsed() > stream_ready_timeout {
            panic!("Timed out waiting for stream to become ready");
        }
    }

    let mut since_cleanup = Instant::now();
    let mut n_sources_playing_prev = 0;
    let mut quit = false;

    loop {
        match pa_mainloop.iterate(false) {
            libpulse_binding::mainloop::standard::IterateResult::Success(_) => (),
            libpulse_binding::mainloop::standard::IterateResult::Quit(_) => {
                log::log!(log::Level::Error, "PulseAudio quit, shutting down");
                break;
            }
            libpulse_binding::mainloop::standard::IterateResult::Err(e) => {
                log::log!(log::Level::Error, "PulseAudio error: {e}, shutting down");
                break;
            }
        }

        // FIXME: what if sleeping here causes underflow on the sound server?
        //        simple way to cause the issue is to play a very large number of
        //        sources simultaneously. maybe the timeout value here should be a
        //        parameter, or adjusted based on load
        match recv_all(&rx, Duration::from_millis(2)) {
            Ok(Some(messages)) => {
                for message in messages {
                    match message {
                        Message::Shutdown => {
                            quit = true;
                            break;
                        }
                        Message::DropAll => sourcegroups.borrow_mut().clear(),
                        Message::PlaySymphoniaSource(sf) => {
                            let _ = sourcegroups
                                .borrow_mut()
                                .entry(sf.spec())
                                .or_insert(SourceGroup::new(
                                    sf.spec(),
                                    output_spec,
                                    conversion_quality,
                                ))
                                .add_source(Source::SymphoniaSource(sf));
                        }
                        Message::CreatePulledSource(setup) => {
                            let _ = sourcegroups
                                .borrow_mut()
                                .entry(setup.spec)
                                .or_insert(SourceGroup::new(
                                    setup.spec,
                                    output_spec,
                                    conversion_quality,
                                ))
                                .add_source(Source::PulledSource(PulledSource::from_setup(setup)));
                        }
                        Message::GetOutputSpec(reply_tx) => match reply_tx.send(output_spec) {
                            Ok(_) => (),
                            Err(e) => {
                                log::log!(log::Level::Error, "Failed to provide output spec: {e}");
                            }
                        },
                    }
                }
            }

            Ok(None) => (),
            Err(ChannelDisconnectedError) => {
                log::log!(
                    log::Level::Error,
                    "Message channel disconnected, shutting down"
                );
                break;
            }
        }

        if quit {
            break;
        }

        sourcegroups
            .borrow_mut()
            .iter_mut()
            .flat_map(|(_spec, group)| group.sources_iter_mut())
            .filter_map(|source| match source {
                Source::PulledSource(ps) => Some(ps),
                _ => None,
            })
            .for_each(|source| source.update());

        if since_cleanup.elapsed().as_millis() >= 1000 {
            since_cleanup = Instant::now();

            for (_spec, group) in sourcegroups.borrow_mut().iter_mut() {
                group.drop_completed_sources();
            }

            let n_sources_playing = sourcegroups
                .borrow()
                .iter()
                .map(|(_spec, group)| group.sources_len())
                .sum::<usize>();

            if n_sources_playing != n_sources_playing_prev {
                log::log!(log::Level::Debug, "{} sources playing", n_sources_playing);

                n_sources_playing_prev = n_sources_playing;
            }
        }
    }

    log::log!(log::Level::Info, "Audiothread shutting down gracefully");

    stream
        .disconnect()
        .expect("We should be able to disconnect from PulseAudio");

    pa_context.disconnect();

    // is this needed/beneficial?
    pa_mainloop.quit(PulseRetval(0));
}

#[cfg(test)]
mod tests {
    use types::Samplerate;

    use super::*;

    #[test]
    fn test_opts() {
        let opts = Opts::default();

        let opts = opts.with_name("Sound Effects");
        let opts = opts.with_spec(AudioSpec::new(22500, 2).unwrap());
        let opts = opts.with_conversion_quality(Quality::Medium);
        let opts = opts.with_buffer_size(NonZeroNumFrames::new(31415).unwrap());

        assert_eq!(opts.stream_name, "Sound Effects");
        assert_eq!(opts.spec.samplerate, Samplerate::new(22500).unwrap());
        assert_eq!(opts.conversion_quality, Quality::Medium);
        assert_eq!(opts.buffer_size, NonZeroNumFrames::new(31415).unwrap());

        let opts = Opts::default()
            .with_name("Background Music")
            .with_conversion_quality(Quality::High);

        assert_eq!(opts.stream_name, "Background Music");
        assert_eq!(opts.conversion_quality, Quality::High);
    }

    #[test]
    fn test_recv_all() {
        fn fmt_message(x: Message) -> String {
            match x {
                Message::Shutdown => String::from("Shutdown"),
                Message::DropAll => String::from("DropAll"),
                _ => String::from(""),
            }
        }

        let (tx, rx) = mpsc::channel::<Message>();

        assert!(recv_all(&rx, Duration::from_millis(0)).is_ok_and(|m| m.is_none()));

        tx.send(Message::DropAll).unwrap();
        tx.send(Message::Shutdown).unwrap();
        tx.send(Message::DropAll).unwrap();

        let x = recv_all(&rx, Duration::from_millis(0))
            .unwrap()
            .unwrap()
            .into_iter()
            .map(fmt_message)
            .collect::<Vec<String>>();

        assert_eq!(x, vec!["DropAll", "Shutdown", "DropAll"]);
        assert!(recv_all(&rx, Duration::from_millis(0)).is_ok_and(|m| m.is_none()));
    }
}
