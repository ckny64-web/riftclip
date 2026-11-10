//! WASAPI process loopback: captures only what the target process plays.

use super::encoder::{AUDIO_BLOCK, AUDIO_RATE};
use super::{qpc_100ns, Shared};
use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use wasapi::{AudioClient, Direction, SampleType, StreamMode, WaveFormat};

/// The audio device lives on its own thread (COM objects stay where they were
/// made). `open` waits until the device is ready, `begin` starts capturing.
pub(crate) struct AudioThread {
    go: Sender<Arc<Shared>>,
    thread: JoinHandle<()>,
}

impl AudioThread {
    pub fn open(pid: u32) -> Result<Self, String> {
        let (ready_tx, ready_rx) = channel::<Result<(), String>>();
        let (go_tx, go_rx) = channel::<Arc<Shared>>();
        let thread = std::thread::Builder::new()
            .name("riftclip-audio".into())
            .spawn(move || {
                let _ = wasapi::initialize_mta();
                let opened = (|| -> Result<_, String> {
                    // The format the game mixes in; converted to 16-bit below.
                    let format = WaveFormat::new(32, 32, SampleType::Float, AUDIO_RATE as usize, 2, None);
                    let mut client = AudioClient::new_application_loopback_client(pid, true)
                        .map_err(|e| format!("open process audio: {e}"))?;
                    let mode = StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: 0 };
                    client
                        .initialize_client(&format, Direction::Capture, mode, None)
                        .map_err(|e| format!("initialize process audio: {e}"))?;
                    let event = client.set_get_eventhandle().map_err(|e| format!("audio event: {e}"))?;
                    let capture = client.get_audiocaptureclient().map_err(|e| format!("audio capture: {e}"))?;
                    Ok((client, event, capture))
                })();
                let (client, event, capture) = match opened {
                    Ok(v) => {
                        let _ = ready_tx.send(Ok(()));
                        v
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                // Dropped sender means the recording never started.
                let Ok(shared) = go_rx.recv() else { return };
                if client.start_stream().is_err() {
                    return;
                }

                let origin = qpc_100ns(); // audio time zero on the shared clock
                let base = origin - shared.start; // its position in the recording
                let mut written: i64 = 0; // audio frames sent to the encoder
                let mut float_bytes: VecDeque<u8> = VecDeque::new();
                let pts = |frames: i64| base + frames * 10_000_000 / AUDIO_RATE as i64;

                while !shared.stop.load(Ordering::Relaxed) {
                    // Wake at least every 100 ms even when the game is silent.
                    let _ = event.wait_for_event(100);

                    while let Ok(Some(n)) = capture.get_next_packet_size() {
                        if n == 0 || capture.read_from_device_to_deque(&mut float_bytes).is_err() {
                            break;
                        }
                    }

                    let frames = float_bytes.len() / 8; // 2 channels x 4 bytes
                    if frames > 0 {
                        let mut pcm = Vec::with_capacity(frames * AUDIO_BLOCK as usize);
                        let mut sample = [0u8; 4];
                        for _ in 0..frames * 2 {
                            for b in &mut sample {
                                *b = float_bytes.pop_front().unwrap_or(0);
                            }
                            let v = (f32::from_le_bytes(sample).clamp(-1.0, 1.0) * 32767.0) as i16;
                            pcm.extend_from_slice(&v.to_le_bytes());
                        }
                        if shared.encoder.write_audio(&pcm, pts(written)).is_err() {
                            return;
                        }
                        written += frames as i64;
                    }

                    // Loopback delivers nothing while the process is silent.
                    // Fill the gap so audio stays in step with the video and
                    // the muxer is never left waiting for this track.
                    let expected = (qpc_100ns() - origin) * AUDIO_RATE as i64 / 10_000_000;
                    let behind = expected - written;
                    if behind > AUDIO_RATE as i64 / 10 {
                        let fill = behind - AUDIO_RATE as i64 / 50; // leave 20 ms of slack
                        let silence = vec![0u8; fill as usize * AUDIO_BLOCK as usize];
                        if shared.encoder.write_audio(&silence, pts(written)).is_err() {
                            return;
                        }
                        written += fill;
                    }
                }
                let _ = client.stop_stream();
            })
            .map_err(|e| format!("start audio thread: {e}"))?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(AudioThread { go: go_tx, thread }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("audio thread exited before it was ready".into()),
        }
    }

    pub fn begin(self, shared: Arc<Shared>) -> Self {
        let _ = self.go.send(shared);
        self
    }

    pub fn join(self) {
        drop(self.go);
        let _ = self.thread.join();
    }
}
