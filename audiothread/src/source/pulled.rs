// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use std::sync::mpsc::{channel, Receiver, Sender};

use ringbuf::{
    traits::{Consumer, Observer},
    HeapCons,
};

use crate::{
    source::SourceOps,
    types::{AudioSpec, NumFrames, StreamState},
};

#[derive(Debug, Clone)]
pub struct PulledSourcePullRequest {
    pub response_tx: Sender<PulledSourcePullReply>,
}

#[derive(Debug, Clone)]
pub enum PulledSourcePullReply {
    FramesProvided(NumFrames),
    Disconnect,
}

pub struct PulledSourceSetup {
    pub name: String,
    pub spec: AudioSpec,
    pub buffer_rx: HeapCons<f32>,
    pub pull_request_tx: Sender<PulledSourcePullRequest>,
}

impl std::fmt::Debug for PulledSourceSetup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!(
            "PulledSourceSetup(name={}, spec={:?}, buffer capacity: {})",
            self.name,
            self.spec,
            self.buffer_rx.capacity()
        ))
    }
}

impl PulledSourceSetup {
    pub fn new(
        name: impl Into<String>,
        spec: AudioSpec,
        buffer_rx: HeapCons<f32>,
        pull_request_tx: Sender<PulledSourcePullRequest>,
    ) -> Self {
        Self {
            name: name.into(),
            spec,
            buffer_rx,
            pull_request_tx,
        }
    }
}

pub(crate) struct PulledSource {
    name: String,
    spec: AudioSpec,
    stream_state: StreamState,
    buffer_rx: HeapCons<f32>,
    pull_request_tx: Sender<PulledSourcePullRequest>,
    pull_response_tx: Sender<PulledSourcePullReply>,
    pull_response_rx: Receiver<PulledSourcePullReply>,
    pull_req_pending: bool,
}

impl std::fmt::Debug for PulledSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!(
            "PulledSource(name={}, spec={:?}, stream_state={:?}, buffer: {} of {}, \
                pending pull request: {})",
            self.name,
            self.spec,
            self.stream_state,
            self.buffer_rx.occupied_len(),
            self.buffer_rx.capacity(),
            self.pull_req_pending,
        ))
    }
}

impl PulledSource {
    pub fn from_setup(setup: PulledSourceSetup) -> Self {
        let (pull_response_tx, pull_response_rx) = channel::<PulledSourcePullReply>();
        Self {
            name: setup.name,
            spec: setup.spec,
            stream_state: StreamState::Streaming,
            buffer_rx: setup.buffer_rx,
            pull_request_tx: setup.pull_request_tx,
            pull_response_tx,
            pull_response_rx,
            pull_req_pending: false,
        }
    }

    pub fn update(&mut self) {
        if let StreamState::Streaming = self.stream_state {
            if self.pull_req_pending {
                match self.pull_response_rx.try_recv() {
                    Ok(reply) => match reply {
                        PulledSourcePullReply::FramesProvided(_n) => self.pull_req_pending = false,
                        PulledSourcePullReply::Disconnect => {
                            log::log!(
                                log::Level::Debug,
                                "Pulled source '{}' disconnected gracefully",
                                self.name
                            );
                            self.stream_state = StreamState::Complete;
                        }
                    },
                    Err(e) => match e {
                        std::sync::mpsc::TryRecvError::Empty => (),
                        std::sync::mpsc::TryRecvError::Disconnected => {
                            log::log!(
                                log::Level::Error,
                                "Pull request response channel broken for source '{}'",
                                self.name
                            );
                            self.stream_state = StreamState::Complete;
                        }
                    },
                }
            } else if self.fraction_filled() < 0.5 {
                self.send_pull_request();
            }
        }
    }

    fn fraction_filled(&self) -> f32 {
        self.buffer_rx.occupied_len() as f32 / self.buffer_rx.capacity().get() as f32
    }

    fn send_pull_request(&mut self) {
        match self.pull_request_tx.send(PulledSourcePullRequest {
            response_tx: self.pull_response_tx.clone(),
        }) {
            Ok(_) => self.pull_req_pending = true,
            Err(_) => {
                log::log!(
                    log::Level::Error,
                    "Pulled source '{}' channel disconnected unexpectedly",
                    self.name
                );
                self.stream_state = StreamState::Complete;
            }
        }
    }
}

impl SourceOps for PulledSource {
    fn spec(&self) -> AudioSpec {
        self.spec
    }

    fn stream_state(&self) -> StreamState {
        self.stream_state
    }

    fn mix_to_same_spec(&mut self, out_buffer: &mut [f32]) {
        let self_chans = self.spec.channels.get() as usize;

        debug_assert!(self.buffer_rx.occupied_len() % self_chans == 0);

        out_buffer
            .iter_mut()
            .zip(self.buffer_rx.pop_iter())
            .for_each(|(output, sample)| *output += sample);
    }
}
