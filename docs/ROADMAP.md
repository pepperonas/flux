# FLUX — Roadmap

Each milestone ends with `cargo check`, `cargo test` and `cargo run` all working
and the application in a state a person can actually play. A milestone is done
when someone unfamiliar can use it, not when it compiles.

The ordering merges the original phase plan (brief §33) with the Phase-2
priority list (§21). Where they disagree, the reason is stated.

---

## M1 — Foundation, sound, loop  ← current target

### Implementation status (2026-09-12)

The transport, bar-quantized four-track event looper, recording click,
interactive performance controls, MIDI note input, and reconnect-safe raw
Xplorer reader are implemented on branch `m1b-looper`. The read-only patch view,
device selection, help overlay, and device-list refresh are now implemented.
Xplorer report bytes are exposed for hardware mapping; musical control mapping
remains M3.

Audio stream failures are published to the interface and retried once per
second. A failed preferred output may fall back to the system default rather
than leaving the application silent.

**Goal: press a key, hear a synth, record a loop that stays in time.**
This is the brief's §39 definition of done, built on the Phase-2 architecture
rather than on something that would need replacing.

* Cargo project, window, dark theme, view shell (Performance / Patch / Settings)
* Event and control layer complete: `ControlId` · `ControlValue` · `ControlEvent`
  · `Action` · `Mapping` · `resolve()` — with the keyboard as the first source
* Parameter registry, smoothing, modulation matrix, macro definitions
* Graph infrastructure: `Module` trait, typed ports, signal-compatibility matrix,
  cycle detection, topological scheduling, buffer pool
* Two-zone engine with a **fixed** default patch: OSC → FILTER → VCA (×16) →
  MIXER → DELAY → REVERB → OUTPUT, plus LFO and envelopes
* PolyBLEP oscillators, TPT state-variable filter, ADSR, voice allocator
* CPAL host: device enumeration, device selection, latency display
* Transport, click, sample-accurate position
* Event looper: 4 tracks, arm/record/overdub/undo/mute/clear, bar-quantized
  length, `1/16` default quantization
* Performance view: loop lanes with live position, BPM, key/scale, three macro
  knobs (BRIGHT · CHAOS · WET), active-note feedback, output meter
* Patch view, read-only: real modules, real connections, live signal activity
* Diagnostics view with latency, load, voices, dropped events, underruns
* Help overlay (`?`)

**Not in M1:** MIDI, guitar, drums, patterns, scenes, cable editing, presets on
disk, onboarding.

M1 deliberately depends on **no external hardware**. It must be fully playable on
a bare laptop, both because that is the honest zero-setup path and because it
must not be blocked waiting for a delivery.

**Done when:** launch → play the computer keyboard → hear a convincing synth →
press record → play → press record → the loop runs in time → record a second
track over it → both stay locked. No mouse required for any of it.

---

## M2 — Control stack: MIDI, surfaces, mapping, learn, macros

Phase-2 §21 items 2–5. Only now does mapping have something worth mapping to.

**MIDI is bidirectional here.** An earlier revision of this roadmap deferred MIDI
output on the grounds that nothing consumed it. The Launchkey Mini MK4 does —
pads, screen and encoder positions are all output — so that decision is reversed.

* `midir` input **and output**; device list in Settings
  - Current: MIDI input, automatic port reconnect, and provisional Launchkey
    encoder/pad input mappings. Output and protocol feedback remain open.
* `ControlSurface` trait: connect / render / disconnect, with per-control rate
  limiting and a guaranteed clean disconnect, including on panic
* **Launchkey Mini MK4 profile** — DAW-mode handshake, 16 RGB pads showing loop
  and drum state, 8 encoders driven relative onto the macros, parameter names on
  the OLED, touch-to-reveal, key and scale pushed to the device
* MIDI clock output, so the device's hardware-synced pad flashing lands on the
  beat for free
* Mapping UI: what is bound to what, change it, remove it
* **Learn mode**: press Learn → move any control (key, CC, pad, later a fret) →
  FLUX names it → pick a target → save. One implementation covers every source,
  because they all speak `ControlId`. Chord-bound notes already remember their
  concrete note and release correctly after their modifier is lifted.
* Full macro set: BRIGHT · DARK · WET · DRY · ENERGY · CHAOS · DENSITY · SPACE,
  unipolar or bipolar
* Mappings, macro assignments and device profiles persist to disk
* **Parameter smoothing wired into the DSP path** (ARCHITECTURE §4). The
  one-pole exists and is used; what M1a does not have is a per-sample parameter
  value for the DSP to read, and nothing before M2 can jump a parameter hard
  enough to need one
* **Voice stealing at unequal velocity** (ARCHITECTURE §6). The handover carries
  the stolen voice's filter and its envelope but not the gain they feed, so a
  theft between two notes struck at different strengths still steps — and in the
  quiet-stolen-by-loud direction it steps ten times harder than the cut the
  handover replaced. Latent on the computer keyboard, ordinary the moment every
  note carries its own velocity. Ships with a stealing test at unequal velocity

Built against the published protocol and accepted on the physical device, which
is expected within days. Protocol details are in
[CONTROLLER_MAPPING.md](CONTROLLER_MAPPING.md), including the parts that are
still assumed rather than verified.

**Done when:** the Launchkey's pads show what the looper is doing, an encoder
moves a macro without a jump, the OLED names it, and a mapping made by moving a
knob survives a restart — none of it requiring a file to be edited.

---

## M3 — The guitar as an instrument

Phase-2 §21 item 1 completed for hardware, plus item 9.

* Raw USB reader thread (`rusb`, interface 0, ~1 kHz), reconnect-safe
* Controller setup wizard: learns frets, strum, whammy, tilt by diffing reports
* Controller debug view with live values
* Interaction model: `InputChord` — hold fret + strum, modifier + fret, fret +
  whammy — fully remappable
* Whammy and tilt as continuous modulation sources through the mod matrix
* Hot-plug: connect and disconnect mid-performance without touching audio

**Done when:** the guitar is unplugged mid-loop and the music does not falter.

---

## M4 — Patchable graph and Patch View editing

Phase-2 §21 items 6–7. Deliberately after the control stack: an editable patch is
worth far more once there is something to modulate it with.

* Cable editing: drag output → input, invalid connections refused with a reason
* Add, remove and move modules; live re-compile of the schedule
* Animated signal flow, visible modulation depth
* Patch save/load

---

## M5 — Drums and patterns

* Synthesised drum voices: kick, snare, closed hat, open hat, clap, perc
* 16/32-step patterns, live triggerable
* Generative pattern engine constrained by scale, key, octave, density,
  complexity, rhythm — **musical safety** (§15): the default range cannot
  produce unusable results; an advanced mode widens it
* `CHAOS` and `DENSITY` wired to pattern variation

---

## M6 — Scenes, presets, live safety

* Scenes capture patch, params, macros, patterns, mapping, BPM
* Quantized scene changes, interpolated BPM changes
* Confirmation or held modifier on every destructive action
* Factory presets: Ambient · Cyber · Industrial · Synthwave · Glitch · Minimal ·
  Dark · Retro · Experimental — all synthesised, no third-party samples
* User presets separated from factory presets

---

## M7 — Polish

* Onboarding (skippable), interactive help, contextual tooltips everywhere
* Visual refinement: motion, glow, beat pulse, spectrum
* Sampler with user files (`symphonia`)
* Windows build and packaging; Linux afterwards
* Accessibility and performance passes

---

## Deferred, with reasons

| Item | Why not yet |
|---|---|
| Audio-rate looper tracks | Needs microphone input; event tracks cover the current goal |
| Sub-block command timestamps | Only matters once the guitar lands; the field already exists |
| MIDI clock **input** / external sync | Requires the transport to follow an external master — a design of its own. Clock *output* ships in M2 |
| Launchkey Custom Modes | The stock DAW-mode surface covers the current needs; custom modes are a second mapping layer to design later |
| Cargo workspace split | Mechanical when it becomes worthwhile; no benefit today |
