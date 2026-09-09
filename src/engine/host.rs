use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::audio::engine::AudioEngine;
use crate::engine::telemetry::Telemetry;

#[derive(Debug, thiserror::Error)]
pub enum HostError {
    #[error("no audio output device is available")]
    NoDevice,
    #[error("the audio device did not offer a usable configuration: {0}")]
    NoConfig(String),
    #[error("the audio stream could not be started: {0}")]
    Stream(String),
}

/// The buffer size we ask the device for. An instrument that answers late is
/// not an instrument, so we request something small; if the device refuses,
/// `start` retries with whatever it prefers instead of refusing to make sound.
const REQUESTED_BUFFER_FRAMES: u32 = 256;

/// How long `start` waits, once the stream is playing, for the device to
/// actually call back before falling back to `REQUESTED_BUFFER_FRAMES` as a
/// last-resort estimate. This only matters on the fallback path, where the
/// stream is built with `BufferSize::Default` and CPAL does not expose what
/// the host picked ahead of time - the true number of frames per callback is
/// only knowable by watching a real callback arrive.
const FIRST_CALLBACK_TIMEOUT: Duration = Duration::from_millis(500);
const FIRST_CALLBACK_POLL: Duration = Duration::from_millis(1);

/// Owns the CPAL stream. The stream must not outlive this value and, on some
/// platforms, must stay on the thread that built it - so `AudioHost` is held by
/// the application and never sent between threads.
pub struct AudioHost {
    _stream: cpal::Stream,
    pub device_name: String,
    pub sample_rate: f32,
    pub buffer_frames: u32,
    pub telemetry: Arc<Telemetry>,
}

impl AudioHost {
    pub fn devices() -> Vec<String> {
        let host = cpal::default_host();
        host.output_devices()
            .map(|ds| ds.filter_map(|d| d.name().ok()).collect())
            .unwrap_or_default()
    }

    pub fn latency_ms(&self) -> f32 {
        self.buffer_frames as f32 / self.sample_rate * 1000.0
    }

    pub fn start(preferred: Option<&str>) -> Result<AudioHost, HostError> {
        let host = cpal::default_host();
        let device = match preferred {
            Some(name) => host
                .output_devices()
                .ok()
                .and_then(|mut ds| ds.find(|d| d.name().map(|n| n == name).unwrap_or(false)))
                .or_else(|| host.default_output_device()),
            None => host.default_output_device(),
        }
        .ok_or(HostError::NoDevice)?;

        let device_name = device.name().unwrap_or_else(|_| "unknown".into());
        let config = device
            .default_output_config()
            .map_err(|e| HostError::NoConfig(e.to_string()))?;
        let sample_rate = config.sample_rate().0 as f32;
        let channels = config.channels() as usize;

        // Ask for a small buffer. An instrument that answers late is not an
        // instrument. If the device refuses, CPAL falls back to its default and
        // the diagnostics view will show the latency actually achieved.
        let mut stream_config: cpal::StreamConfig = config.clone().into();
        stream_config.buffer_size = cpal::BufferSize::Fixed(REQUESTED_BUFFER_FRAMES);

        let telemetry = Telemetry::new(1024);
        let mut engine = AudioEngine::new(sample_rate, 2048, Arc::clone(&telemetry));

        // `BufferSize::Fixed` is a request the device can round or ignore, and
        // the fallback path below asks for `BufferSize::Default`, whose actual
        // frame count CPAL does not expose ahead of time. Either way the only
        // ground truth for "how many frames does a callback actually carry" is
        // a callback that has actually happened, so the first one records it
        // here instead of us assuming a number.
        let observed_frames = Arc::new(AtomicU32::new(0));

        let cb_telemetry = Arc::clone(&telemetry);
        let cb_observed = Arc::clone(&observed_frames);
        let build = device.build_output_stream(
            &stream_config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let started = std::time::Instant::now();
                let frames = data.len() / channels;
                cb_observed.store(frames as u32, Ordering::Relaxed);
                engine.render(frames);
                let out = engine.output();
                for (i, frame) in data.chunks_mut(channels).enumerate() {
                    let v = out.get(i).copied().unwrap_or(0.0);
                    for sample in frame.iter_mut() {
                        *sample = v;
                    }
                }
                let budget = frames as f32 / sample_rate;
                let used = started.elapsed().as_secs_f32();
                cb_telemetry.set_dsp_load(((used / budget) * 1000.0) as u32);
            },
            move |err| {
                // A device error must never take the process down. The interface
                // reports it; the player's loops keep running.
                log::error!("audio stream error: {err}");
            },
            None,
        );

        let stream = match build {
            Ok(s) => s,
            // The fixed buffer size is a request, not a requirement. Retry with
            // whatever the device prefers rather than refusing to make sound.
            Err(_) => {
                let fallback: cpal::StreamConfig = config.into();
                let cb_telemetry2 = Arc::clone(&telemetry);
                let observed2 = Arc::clone(&observed_frames);
                let mut engine2 = AudioEngine::new(sample_rate, 4096, Arc::clone(&telemetry));
                device
                    .build_output_stream(
                        &fallback,
                        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                            // Mirrors the primary callback above: measure and
                            // render the same way, so a device that only
                            // accepts its own default buffer still gets
                            // honest peak and DSP-load telemetry rather than
                            // a fallback path that quietly reports nothing.
                            let started = std::time::Instant::now();
                            let frames = data.len() / channels;
                            observed2.store(frames as u32, Ordering::Relaxed);
                            engine2.render(frames);
                            let out = engine2.output();
                            for (i, frame) in data.chunks_mut(channels).enumerate() {
                                let v = out.get(i).copied().unwrap_or(0.0);
                                for sample in frame.iter_mut() {
                                    *sample = v;
                                }
                            }
                            let budget = frames as f32 / sample_rate;
                            let used = started.elapsed().as_secs_f32();
                            cb_telemetry2.set_dsp_load(((used / budget) * 1000.0) as u32);
                        },
                        move |err| log::error!("audio stream error: {err}"),
                        None,
                    )
                    .map_err(|e| HostError::Stream(e.to_string()))?
            }
        };

        stream
            .play()
            .map_err(|e| HostError::Stream(e.to_string()))?;

        // Wait for the device to actually report a frame count rather than
        // trusting the request: `Fixed` can be rounded by the device and
        // `Default` (the fallback path) carries no size information at all
        // until a callback happens.
        let buffer_frames = wait_for_observed_frames(&observed_frames);

        Ok(AudioHost {
            _stream: stream,
            device_name,
            sample_rate,
            buffer_frames,
            telemetry,
        })
    }
}

/// Polls `observed` until a real callback has recorded a frame count or the
/// timeout elapses, in which case `REQUESTED_BUFFER_FRAMES` stands in as the
/// best available estimate. This runs once, during application start-up, off
/// the audio thread - never inside a callback.
fn wait_for_observed_frames(observed: &AtomicU32) -> u32 {
    let deadline = std::time::Instant::now() + FIRST_CALLBACK_TIMEOUT;
    loop {
        let frames = observed.load(Ordering::Relaxed);
        if frames != 0 {
            return frames;
        }
        if std::time::Instant::now() >= deadline {
            return REQUESTED_BUFFER_FRAMES;
        }
        std::thread::sleep(FIRST_CALLBACK_POLL);
    }
}
