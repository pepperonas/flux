# Changelog

## 0.1.0 — M1b foundation

The binary reports this version with `--version` / `-V`.

Transport Stop now sends all active voices into their release phase, including
notes started by loop playback.

- Real-time synth engine with transport, recording click, and four-track event
  looper.
- Performance, read-only patch, diagnostics, and settings views.
- Runtime audio-device selection and refresh; MIDI reconnect action.
- MIDI note input, provisional encoder CC 21–28 macro mapping, and diagnostics.
- Reconnect-safe raw Xplorer USB reader with report fingerprint, byte display,
  and report-difference learner.
- Automatic MIDI port change detection and visible MIDI/Xplorer connection
  indicators in Diagnostics.
- Automatic audio-stream recovery with a visible running/recovering indicator.
- MIDI discovery errors no longer tear down healthy existing connections.
- Provisional Launchkey channel-10 pads 44–46 now control transport toggle,
  stop, and play.
- Voice-steal velocity handover and parameter smoothing in the DSP path.
- Correct note-off handling for chord-bound notes after modifier release.
- Sample-offset loop playback: note events now enter the voice engine at their
  exact position inside each audio block, including loop wrap-around.

The Xplorer control mapping and Launchkey protocol details remain hardware
verification tasks for M2/M3.
