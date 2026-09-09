use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

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

/// The buffer size actually requested from the device. An instrument that
/// answers late is not an instrument, so we ask for something small; if the
/// device refuses, `start` retries with whatever it prefers instead of
/// refusing to make sound. This is a request only - see `ObservedBufferSize`
/// for what the device is actually delivering.
const REQUESTED_BUFFER_FRAMES: u32 = 256;

/// The number of frames a callback actually delivers, filled in by the first
/// callback that happens. Neither `BufferSize::Fixed` (a request the device
/// can round) nor `BufferSize::Default` (used on the fallback path, and
/// completely opaque to CPAL's caller ahead of time) tells you the real
/// number before a callback has actually carried one, so this starts unknown
/// rather than guessing.
///
/// Zero is the "not yet observed" sentinel. This does not collide with a
/// real observation in practice - no CPAL backend hands a callback a
/// zero-length buffer - so it is left as a plain `AtomicU32` rather than
/// something heavier; see `get`.
struct ObservedBufferSize(AtomicU32);

impl ObservedBufferSize {
    fn new() -> ObservedBufferSize {
        ObservedBufferSize(AtomicU32::new(0))
    }

    /// Called from the audio callback. Never blocks, never allocates.
    fn record(&self, frames: u32) {
        self.0.store(frames, Ordering::Relaxed);
    }

    /// `None` until a callback has actually happened. The point of this
    /// being `Option` rather than a `u32` with an implied "0 means unknown"
    /// convention is that a caller cannot accidentally format the unknown
    /// state as though it were a measurement.
    fn get(&self) -> Option<u32> {
        match self.0.load(Ordering::Relaxed) {
            0 => None,
            n => Some(n),
        }
    }
}

/// Owns the CPAL stream. The stream must not outlive this value and, on some
/// platforms, must stay on the thread that built it - so `AudioHost` is held by
/// the application and never sent between threads.
pub struct AudioHost {
    _stream: cpal::Stream,
    pub device_name: String,
    pub sample_rate: f32,
    buffer_frames: Arc<ObservedBufferSize>,
    pub telemetry: Arc<Telemetry>,
}

impl AudioHost {
    pub fn devices() -> Vec<String> {
        let host = cpal::default_host();
        host.output_devices()
            .map(|ds| ds.filter_map(|d| d.name().ok()).collect())
            .unwrap_or_default()
    }

    /// The number of frames the device is actually delivering per callback.
    /// `None` until the first callback has happened - a silent device or one
    /// that vanished between `build_output_stream` and `play` never reports
    /// anything else, which is the point: the diagnostics view can show that
    /// honestly instead of a plausible-looking guess.
    pub fn buffer_frames(&self) -> Option<u32> {
        self.buffer_frames.get()
    }

    /// Milliseconds of output latency implied by the last-observed buffer
    /// size. `None` under exactly the same condition as `buffer_frames`.
    pub fn latency_ms(&self) -> Option<f32> {
        self.buffer_frames()
            .map(|frames| frames as f32 / self.sample_rate * 1000.0)
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

        // Published asynchronously by whichever callback actually runs, and
        // read on demand by `buffer_frames`/`latency_ms`. `start` does not
        // wait for it: a stream that never calls back should be visible as
        // "still measuring", not turn into a blocked application start-up.
        let observed_frames = Arc::new(ObservedBufferSize::new());

        let cb_telemetry = Arc::clone(&telemetry);
        let cb_observed = Arc::clone(&observed_frames);
        let build = device.build_output_stream(
            &stream_config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let started = std::time::Instant::now();
                let frames = data.len() / channels;
                cb_observed.record(frames as u32);
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
                            observed2.record(frames as u32);
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

        Ok(AudioHost {
            _stream: stream,
            device_name,
            sample_rate,
            buffer_frames: observed_frames,
            telemetry,
        })
    }
}

/// Renders a buffer-size measurement for the diagnostics view, honest about
/// not knowing it yet rather than showing a guess formatted like a fact.
pub fn describe_buffer_frames(frames: Option<u32>) -> String {
    match frames {
        Some(n) => format!("{n} frames"),
        None => "measuring…".to_string(),
    }
}

/// Renders a latency measurement for the diagnostics view. See
/// `describe_buffer_frames`.
pub fn describe_latency_ms(latency_ms: Option<f32>) -> String {
    match latency_ms {
        Some(ms) => format!("{ms:.1} ms"),
        None => "measuring…".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_size_is_unknown_until_the_first_callback() {
        let observed = ObservedBufferSize::new();
        assert_eq!(observed.get(), None);
    }

    #[test]
    fn buffer_size_is_known_once_a_callback_has_recorded_one() {
        let observed = ObservedBufferSize::new();
        observed.record(512);
        assert_eq!(observed.get(), Some(512));
    }

    #[test]
    fn a_later_callback_updates_the_observed_size() {
        // Not expected to change block-to-block in practice, but nothing here
        // assumes it can't; recording just publishes the latest value.
        let observed = ObservedBufferSize::new();
        observed.record(256);
        observed.record(512);
        assert_eq!(observed.get(), Some(512));
    }

    #[test]
    fn the_unknown_state_cannot_be_formatted_as_a_number() {
        let frames_text = describe_buffer_frames(None);
        assert!(
            frames_text.chars().all(|c| !c.is_ascii_digit()),
            "the 'unknown' buffer rendering contained a digit: {frames_text:?}"
        );
        assert!(frames_text.parse::<u32>().is_err());

        let latency_text = describe_latency_ms(None);
        assert!(
            latency_text.chars().all(|c| !c.is_ascii_digit()),
            "the 'unknown' latency rendering contained a digit: {latency_text:?}"
        );
        assert!(latency_text.parse::<f32>().is_err());
    }

    #[test]
    fn the_known_state_formats_as_a_real_number() {
        assert_eq!(describe_buffer_frames(Some(512)), "512 frames");
        assert_eq!(describe_latency_ms(Some(11.6)), "11.6 ms");
    }
}
