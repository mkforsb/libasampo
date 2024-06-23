// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use audiothread::*;
use ringbuf::{
    traits::{Observer, Producer, Split},
    HeapRb,
};

struct VibratoSaw {
    samplerate: f32,
    freq: f32,
    offset: f32,
    vib_phase: f32,
}

impl Iterator for VibratoSaw {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let signal = 1.0f32 - 2.0f32 * (self.offset % self.samplerate) / self.samplerate;
        self.offset += self.freq + 4.0 * self.vib_phase.sin();
        self.vib_phase += 2.0 * std::f32::consts::PI * 4.0 / self.samplerate;
        Some(0.1 * signal)
    }
}

#[test]
#[ignore]
fn test_pulled_source_saw_wave_playback() {
    let (tx, rx) = std::sync::mpsc::channel::<Message>();

    let _audiothread = spawn(
        rx,
        Some(Opts::default().with_conversion_quality(Quality::Lowest)),
    );

    let saw_fn = |freq: f32| {
        let psbuf = HeapRb::<f32>::new(44100);
        let (mut psbuf_prod, psbuf_cons) = psbuf.split();

        let (ps_tx, ps_rx) = std::sync::mpsc::channel::<PulledSourcePullRequest>();

        let ps = PulledSourceSetup::new(
            "Saw wave",
            AudioSpec::new(44100, 1).unwrap(),
            psbuf_cons,
            ps_tx,
        );

        let mut saw = VibratoSaw {
            samplerate: 44100.0,
            freq,
            offset: 0.0,
            vib_phase: 0.0,
        };

        std::thread::spawn(move || loop {
            match ps_rx.recv() {
                Ok(req) => {
                    let provided_frames = psbuf_prod.vacant_len();
                    psbuf_prod.push_iter(saw.by_ref());

                    match req.response_tx.send(PulledSourcePullReply::FramesProvided(
                        provided_frames.into(),
                    )) {
                        Ok(_) => (),
                        Err(_) => return,
                    }
                }
                Err(_) => return,
            };
        });

        let _ = tx.send(Message::CreatePulledSource(ps));
    };

    saw_fn(261.63);
    saw_fn(329.63);
    saw_fn(392.0);

    std::thread::sleep(std::time::Duration::from_secs(5));
}
