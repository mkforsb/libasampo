// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

#![cfg(feature = "audiothread-integration")]

use std::{
    collections::VecDeque,
    sync::mpsc::{channel, Receiver, SendError, Sender},
    time::Instant,
};

use ringbuf::{
    traits::{Observer, Producer, Split},
    HeapProd, HeapRb,
};

use crate::{
    errors::Error,
    samplesets::DrumkitLabel,
    sequences::{
        DrumkitSequence, DrumkitSequenceEvent, DrumkitSequenceRenderer, SampleSetSampleLoader,
        Swing, BPM,
    },
};

pub enum Message {
    Play,
    Pause,
    Stop,
    Shutdown,
    LoadSampleSet(SampleSetSampleLoader),
    SetTempo(BPM),
    SetSwing(Swing),
    SetSequence(DrumkitSequence),
    ResetSequence,
    ClearSequence,
    EditSequenceClearStep(usize),
    EditSequenceSetStepTrigger {
        step: usize,
        label: DrumkitLabel,
        amp: f32,
    },
    EditSequenceUnsetStepTrigger {
        step: usize,
        label: DrumkitLabel,
    },
}

struct State {
    renderer: DrumkitSequenceRenderer,
    paused: bool,
    buffer: Vec<f32>,
    buffer_tx: HeapProd<f32>,
    pull_request_rx: Receiver<audiothread::PulledSourcePullRequest>,
    control_rx: Receiver<Message>,
    events: VecDeque<DrumkitSequenceEvent>,
    event_tx: Option<single_value_channel::Updater<Option<DrumkitSequenceEvent>>>,
}

impl State {
    pub fn new(
        audiothread_tx: Sender<audiothread::Message>,
        control_rx: Receiver<Message>,
        event_tx: Option<single_value_channel::Updater<Option<DrumkitSequenceEvent>>>,
    ) -> Result<Self, Error> {
        let (spec_tx, spec_rx) = channel::<audiothread::AudioSpec>();

        audiothread_tx
            .send(audiothread::Message::GetOutputSpec(spec_tx))
            .map_err(|e| Error::ChannelError(e.to_string()))?;

        let output_spec = spec_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .map_err(|e| Error::ChannelError(e.to_string()))?;

        log::log!(log::Level::Debug, "Output spec: {:?}", output_spec);

        let renderer = DrumkitSequenceRenderer::new(output_spec.samplerate.get().try_into()?);

        let (pull_request_tx, pull_request_rx) = channel::<audiothread::PulledSourcePullRequest>();

        let bufsize = ((output_spec.samplerate.get() as usize) / 8) * 2;
        let buffer = vec![0.0f32; bufsize];

        log::log!(
            log::Level::Debug,
            "Drum render buffer size (frames): {}",
            buffer.len() / 2
        );

        let (buffer_tx, buffer_rx) = HeapRb::<f32>::new(bufsize).split();

        audiothread_tx
            .send(audiothread::Message::CreatePulledSource(
                audiothread::PulledSourceSetup::new(
                    "DrumkitSequence",
                    output_spec,
                    buffer_rx,
                    pull_request_tx,
                ),
            ))
            .map_err(|e| Error::ChannelError(e.to_string()))?;

        Ok(Self {
            renderer,
            paused: true,
            buffer,
            buffer_tx,
            pull_request_rx,
            control_rx,
            events: VecDeque::new(),
            event_tx,
        })
    }
}

pub fn spawn(
    audiothread_tx: Sender<audiothread::Message>,
    control_rx: Receiver<Message>,
    event_tx: Option<single_value_channel::Updater<Option<DrumkitSequenceEvent>>>,
) -> std::thread::JoinHandle<()> {
    // TODO: consider switching to crossbeam-channel (or flume?), for "select!"
    std::thread::spawn(move || {
        let mut rts = match State::new(audiothread_tx, control_rx, event_tx) {
            Ok(rts) => rts,
            Err(e) => {
                log::log!(
                    log::Level::Error,
                    "Failed to spawn drumkit sequence render thread: {e}"
                );
                panic!();
            }
        };

        let mut shutdown_request: Option<std::time::Instant> = None;
        let shutdown_timeout = std::time::Duration::from_secs(3);
        let send_events = rts.event_tx.is_some();

        loop {
            match (shutdown_request, rts.control_rx.try_recv()) {
                (None, Ok(message)) => match message {
                    Message::Play => rts.paused = false,
                    Message::Pause => rts.paused = true,
                    Message::Stop => {
                        rts.paused = true;
                        rts.renderer.reset_sequence();
                    }
                    Message::Shutdown => {
                        shutdown_request = Some(std::time::Instant::now());
                    }
                    Message::LoadSampleSet(loader) => {
                        rts.renderer.load_samples_async(loader);
                    }
                    Message::SetTempo(bpm) => rts.renderer.set_tempo(bpm),
                    Message::SetSwing(swing) => rts.renderer.set_swing(swing),
                    Message::SetSequence(seq) => rts.renderer.set_sequence(seq),
                    Message::ResetSequence => rts.renderer.reset_sequence(),
                    Message::ClearSequence => rts.renderer.sequence_clear(),
                    Message::EditSequenceClearStep(n) => rts.renderer.sequence_clear_step(n),
                    Message::EditSequenceSetStepTrigger { step, label, amp } => {
                        rts.renderer.sequence_set_step_trigger(step, label, amp)
                    }
                    Message::EditSequenceUnsetStepTrigger { step, label } => {
                        rts.renderer.sequence_unset_step_trigger(step, label)
                    }
                },

                (Some(_), Ok(_)) => {
                    log::log!(log::Level::Warn, "Message received after shutdown request");
                }

                (_, Err(e)) => match e {
                    std::sync::mpsc::TryRecvError::Empty => (),
                    std::sync::mpsc::TryRecvError::Disconnected => {
                        log::log!(
                            log::Level::Error,
                            "Control channel disconnected unexpectedly"
                        );

                        break;
                    }
                },
            }

            match (shutdown_request, rts.pull_request_rx.try_recv()) {
                (None, Ok(req)) => {
                    let num_vacant = rts.buffer_tx.vacant_len();

                    if !rts.paused {
                        let (_, events) = rts
                            .renderer
                            .render(&mut rts.buffer.as_mut_slice()[..num_vacant]);

                        if send_events {
                            if let Some(events) = events {
                                for event in events {
                                    rts.events.push_back(event);
                                }
                            }
                        }
                    } else {
                        // TODO: add pausing on the audiothread side?
                        rts.buffer[..num_vacant].fill(0.0f32);
                    }

                    rts.buffer_tx.push_slice(&rts.buffer[..num_vacant]);

                    match req
                        .response_tx
                        .send(audiothread::PulledSourcePullReply::FramesProvided(
                            num_vacant.into(),
                        )) {
                        Ok(_) => (),
                        Err(SendError(_)) => {
                            log::log!(
                                log::Level::Error,
                                "Pull response channel disconnected unexpectedly"
                            );

                            break;
                        }
                    }
                }

                (Some(_), Ok(req)) => {
                    let _ = req
                        .response_tx
                        .send(audiothread::PulledSourcePullReply::Disconnect);

                    break;
                }

                (_, Err(e)) => match e {
                    std::sync::mpsc::TryRecvError::Empty => (),
                    std::sync::mpsc::TryRecvError::Disconnected => {
                        log::log!(
                            log::Level::Error,
                            "Pull request channel disconnected unexpectedly"
                        );

                        break;
                    }
                },
            }

            if let Some(request) = shutdown_request {
                if request.elapsed() >= shutdown_timeout {
                    log::log!(
                        log::Level::Warn,
                        "Forcibly shutting down drumkit sequence render thread"
                    );

                    break;
                }
            }

            if send_events {
                while !rts.events.is_empty() {
                    match rts.events.front() {
                        Some(ev) if ev.time <= Instant::now() => {
                            match rts
                                .event_tx
                                .as_ref()
                                .unwrap()
                                .update(Some(rts.events.pop_front().unwrap()))
                            {
                                Ok(_) => (),
                                Err(e) => log::log!(log::Level::Debug, "Failed sending event: {e}"),
                            }
                        }
                        _ => break,
                    }
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(2));
        }

        log::log!(log::Level::Debug, "Exit");
    })
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;

    use crate::{
        prelude::*,
        samplesets::{
            BaseSampleSet, DrumkitLabel, DrumkitLabelling, SampleSet, SampleSetLabelling,
        },
        sequences::{NoteLength, TimeSpec},
        sources::{file_system_source::FilesystemSource, Source},
    };

    fn drumkit() -> (Source, SampleSet) {
        let source = Source::FilesystemSource(FilesystemSource::new(
            format!(
                "{}/test_assets/drumkit",
                env::var("CARGO_MANIFEST_DIR").unwrap()
            ),
            vec!["wav".to_string()],
        ));

        let list = source.list().unwrap();

        let bd = list.iter().find(|s| s.name() == "kick.wav").unwrap();
        let ch = list.iter().find(|s| s.name() == "hihat.wav").unwrap();
        let sd = list.iter().find(|s| s.name() == "snare.wav").unwrap();

        let mut set = SampleSet::BaseSampleSet(BaseSampleSet::new("my set".to_string()));

        set.add_with_hash(bd.clone(), "bd".to_string());
        set.add_with_hash(ch.clone(), "ch".to_string());
        set.add_with_hash(sd.clone(), "sd".to_string());

        let mut labels = DrumkitLabelling::new();

        labels.set(bd.uri().clone(), DrumkitLabel::BassDrum);
        labels.set(ch.uri().clone(), DrumkitLabel::ClosedHihat);
        labels.set(sd.uri().clone(), DrumkitLabel::SnareDrum);

        set.set_labelling(Some(SampleSetLabelling::DrumkitLabelling(labels)));

        (source, set)
    }

    fn drumkit_loader() -> SampleSetSampleLoader {
        let (source, set) = drumkit();

        SampleSetSampleLoader::new(set, vec![source])
    }

    fn basic_beat() -> DrumkitSequence {
        let mut seq = DrumkitSequence::new(
            TimeSpec::new_with_swing(120, 4, 4, 0.0).unwrap(),
            NoteLength::Sixteenth,
        );

        seq.set_step_trigger(0, DrumkitLabel::BassDrum, 0.5);
        seq.set_step_trigger(4, DrumkitLabel::BassDrum, 0.5);
        seq.set_step_trigger(8, DrumkitLabel::BassDrum, 0.5);
        seq.set_step_trigger(12, DrumkitLabel::BassDrum, 0.5);

        for i in 0..16 {
            seq.set_step_trigger(i, DrumkitLabel::ClosedHihat, 0.5);
        }

        seq.set_step_trigger(4, DrumkitLabel::SnareDrum, 0.5);
        seq.set_step_trigger(12, DrumkitLabel::SnareDrum, 0.5);

        seq
    }

    #[test]
    #[ignore]
    fn test_drumkit_playback() {
        let (audiothread_tx, audiothread_rx) = channel::<audiothread::Message>();
        audiothread::spawn(audiothread_rx, Some(audiothread::Opts::default()));

        let (control_tx, control_rx) = channel::<Message>();
        spawn(audiothread_tx.clone(), control_rx, None);

        let _ = control_tx.send(Message::LoadSampleSet(drumkit_loader()));
        let _ = control_tx.send(Message::SetSequence(basic_beat()));
        let _ = control_tx.send(Message::Play);

        std::thread::sleep(std::time::Duration::from_secs(10));

        let _ = control_tx.send(Message::Shutdown);
        std::thread::sleep(std::time::Duration::from_secs(1));

        let _ = audiothread_tx.send(audiothread::Message::Shutdown);
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
