// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use std::{
    cell::{BorrowMutError, RefCell, RefMut},
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

fn grab<T>(r: Result<RefMut<Option<T>>, BorrowMutError>) -> Option<RefMut<T>> {
    r.map(|x| RefMut::map(x, |y| y.as_mut().unwrap())).ok()
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

    log::log!(log::Level::Info, "Audiothread starting up");

    let mut pa_mainloop =
        PulseMainloop::new().expect("Libpulse should be able to allocate a mainloop");

    let pa_context = Rc::new(RefCell::new(
        PulseContext::new(&pa_mainloop, &opts.stream_name)
            .expect("Libpulse should be able to allocate a context"),
    ));
    let pa_context_csc = Rc::clone(&pa_context);

    let pa_stream: Rc<RefCell<Option<PulseStream>>> = Rc::new(RefCell::new(None));
    let pa_stream_csc = Rc::clone(&pa_stream);
    let pa_stream_ssc = Rc::clone(&pa_stream);
    let pa_stream_srw = Rc::clone(&pa_stream);

    let sourcegroups: Rc<RefCell<HashMap<AudioSpec, SourceGroup>>> =
        Rc::new(RefCell::new(HashMap::new()));

    let sourcegroups_srw = Rc::clone(&sourcegroups);

    let stream_ready_write = move |n: usize| {
        debug_assert!(n % framesize_bytes == 0);

        match grab(pa_stream_srw.try_borrow_mut()) {
            Some(ref mut s) => match s.begin_write(Some(n)) {
                Ok(Some(ref mut buf)) => {
                    // TODO: skip if no sources are playing.
                    //       complication: the buffer received from .begin_write may need to
                    //       to be zeroed once after the last playing source is dropped.
                    unsafe {
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
                    let mut s = PulseStream::new(ctx, "stream", &pulse_spec, None)
                        .expect("Libpulse should be able to allocate a stream");

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
        .connect(None, PulseContextFlagSet::NOAUTOSPAWN, None)
        .expect("We should be able to connect to PulseAudio");

    log::log!(
        log::Level::Info,
        "Connected to server {:?}",
        pa_context.borrow().get_server()
    );

    let mut stream_asked_connect = false;
    let mut stream_created = false;

    let mut since_cleanup = Instant::now();

    let mut n_sources_playing_prev = 0;

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
            ) {
                Ok(_) => stream_asked_connect = true,
                _ => (),
            }
        }

        let mut quit = false;

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
