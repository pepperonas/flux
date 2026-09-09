# FLUX — Architecture

> FLUX is not a DAW. FLUX is a playable music instrument.

This document is the authoritative description of how FLUX is built and *why*.
It is written in English to match the code and the user-facing vocabulary
(`BRIGHT`, `CHAOS`, `PERFORMANCE`), even though project conversation is German.

Status: **initial architecture**, written before the first line of code.
It already incorporates the Phase-2 requirements (control abstraction, MIDI,
mapping, learn mode, macros, modular signal model, patch view) — see
[PHASE2.md](PHASE2.md) for why they were folded in up front rather than retrofitted.

---

## 1. Principles

These are ordered. When two conflict, the higher one wins.

1. **The audio thread never blocks.** No allocation, no lock, no syscall, no
   file I/O, no `String`, no `Drop` that frees. A glitch is worse than a
   missing feature.
2. **All input goes through the control layer.** The audio engine must not be
   able to tell whether an event came from a key, a fret or a MIDI knob.
   `INPUTS → CONTROL EVENTS → MUSICAL ENGINE → AUDIO ENGINE`, never a shortcut.
3. **One mechanism beats three special cases.** Macros, LFOs and mapped
   continuous controls are the same thing: entries in a modulation matrix.
4. **The audio thread is the only clock.** Everything musical is expressed in
   samples. The GUI is a viewer, never a timing authority.
5. **Simple and understandable beats powerful and clever** (project brief §36).
   Complexity is added only when a concrete requirement demands it.
6. **Nothing the user is playing may be destroyed by a disconnect, a dropped
   packet or a mis-click.**

---

## 2. Thread model and data flow

```
   ┌──────────────┐   ControlEvent
   │ GUI thread   │◄──────────────────┐
   │ eframe 60 Hz │                   │
   └──────┬───────┘                   │
          │ AudioCommand              │
          ▼                           │
   ╔══════════════════════╗           │
   ║ ArrayQueue<Command>  ║◄──────────┼────── ┌────────────────┐
   ║ lock-free MPSC       ║           │       │ USB thread     │
   ╚══════════┬═══════════╝           ├───────┤ ~1 kHz blocking│
              │                       │       └────────────────┘
              ▼                       │       ┌────────────────┐
   ┌──────────────────────┐           └───────┤ MIDI callback  │
   │  AUDIO THREAD (RT)   │                   └────────────────┘
   │  owns all DSP state  │
   └──────┬───────────┬───┘
          │           │
   ArrayQueue<        Atomics
   EngineEvent>       (position, levels, voices, drops)
          │           │
          └─────►  GUI thread
```

### Why input sources write to the command queue *directly*

The obvious design routes every input through the GUI thread, which then
resolves the mapping and talks to audio. It is rejected: the GUI runs at vsync,
so guitar and MIDI would inherit up to 16 ms of jitter. That is the difference
between an instrument and software. Input threads therefore resolve the mapping
themselves against a wait-free snapshot (`arc_swap::ArcSwap<Mapping>`) and push
commands at their own rate (~1 ms for USB). A copy of every `ControlEvent` is
additionally sent to the GUI over a bounded `crossbeam-channel` purely for
display and learn mode — if that channel is full, the *display* copy is dropped,
never the musical one.

### Why continuous telemetry uses atomics, not the event queue

Loop position, output level, active-voice count and CPU load change every block.
Pushing them through a queue at ~200 Hz would flood it and starve real events.
They live in atomics that the GUI samples once per frame. The event queue carries
only *discrete* facts (loop wrapped, recording armed, voice stolen, patch changed).

### Command timestamps

`AudioCommand` carries `host_time_ns: u64` from a monotonic clock. The audio
thread maintains a running map from host time to sample position (updated on
every callback) so that an event produced 3 ms ago inside a 10 ms block can be
placed at the correct sample offset. **M1 stamps commands at drain time**
(block-aligned); sub-block placement is switched on when the guitar lands, where
it actually matters. The field exists from the start so that turning it on is
not a signature change through five modules.

### Drop counting

Every failed `push` increments an `AtomicU64`. There is no other way to learn
later that events were lost — you cannot retrofit a counter into a path that
already silently discarded. Surfaced in the diagnostics view (§10).

---

## 3. Control layer

The heart of requirement §2/§4/§20 of the Phase-2 brief.

```rust
enum ControlId {
    Keyboard(KeyCode),
    Guitar { device: DeviceId, control: GuitarControl }, // Fret(0..4), Strum, Whammy, Tilt, Start, Select, Dpad(..)
    Midi   { port: MidiPortId, channel: u4, control: MidiControl }, // Note(u7), Cc(u7), PolyAftertouch(u7), PitchBend
}

enum ControlValue {
    Gate(bool),        // held: key, fret, pad
    Trigger,           // momentary impulse: strum, drum pad
    Continuous(f32),   // 0.0 ..= 1.0: whammy, tilt, CC, fader
    Delta(i32),        // relative encoder
}

struct ControlEvent { id: ControlId, value: ControlValue, host_time_ns: u64 }
```

The MIDI channel is part of the identity, not a detail. On the Launchkey Mini MK4
the channel *is* the namespace: pads report on channel 10, encoders on 16, encoder
touch on 15, device features on 7. Folding the channel away would merge controls
that are physically different things.

`ControlId` is `Copy + Eq + Hash` — that is what makes learn mode nearly free:
"remember the next incoming `ControlId`" is three lines, and it works identically
for a fret, a knob and a key because every source speaks the same identity type.

Discrete vs. continuous (§4) is encoded in the *value*, not in a convention a
future module could get wrong.

### Mapping

```rust
enum Binding {
    Action(Action),                                  // discrete → do something
    Param { target: ParamId, depth: f32, curve: Curve }, // continuous → modulate
    Chord(InputChord, Box<Binding>),                 // modifier combinations
}

struct Mapping {
    bindings: HashMap<ControlId, Vec<Binding>>,
    chords:   Vec<(InputChord, Binding)>,
}
```

An `InputChord` is a set of currently-held `ControlId`s plus a triggering one —
this is how "hold fret + strum", "Green+Red = bass with distortion" and
"modifier + fret = switch pattern" are all expressed by one structure (§8/§10).

Resolution is a **pure function**, and it is the single most heavily tested piece
of the codebase:

```rust
fn resolve(mapping: &Mapping, held: &HeldSet, ev: ControlEvent) -> ActionList
```

No audio, no GUI, no I/O. Every interaction rule in FLUX is a property of this
function and can be asserted in a unit test.

### Actions

```rust
enum Action {
    NoteOn { note: u8, velocity: f32 }, NoteOff { note: u8 },
    Trigger { target: TriggerTarget },        // drum voice, pattern, one-shot
    LoopControl(LoopCmd),                     // Arm, Toggle, Clear, Undo, Mute, Solo
    SceneTrigger { scene: SceneId, quantized: bool },
    PatternTrigger { pattern: PatternId },
    SetMacro { macro_id: MacroId, value: f32 },
    Transport(TransportCmd),                  // Play, Stop, TapTempo, Bpm(f32)
}
```

The audio engine consumes `Action`-derived commands. It has no type in scope
that mentions a key, a fret or a CC number. That is requirement §20, enforced by
the module graph rather than by discipline.

### Control surfaces are bidirectional

An input source that only *produces* events is not enough. A modern controller
carries state: RGB pads, a screen, encoders that must be told where they are.
FLUX therefore has a second trait on top of the source:

```rust
trait ControlSurface: InputSource {
    fn connect(&mut self);                      // handshake, put the device in the right modes
    fn render(&mut self, state: &SurfaceState); // FLUX state → LEDs, screen, encoder positions
    fn disconnect(&mut self);                   // hand the device back to standalone operation
}
```

`SurfaceState` is a small, device-agnostic snapshot — loop track states, current
macro values and names, transport, key and scale, armed track. A surface decides
for itself how to show it. A device with no feedback (the guitar) implements
`render` as a no-op; nothing else in FLUX changes.

`render` runs on the surface's own thread at a modest rate (~30 Hz) and is
rate-limited per control: a MIDI surface that redraws sixteen pads every frame
floods a 31.25 kbaud DIN port and its own USB endpoint. Only changed cells are
sent.

`disconnect` matters more than it looks. A device left in DAW mode after FLUX
exits is a device that behaves oddly in the user's other software. Restoring it
is part of shutting down cleanly, including on panic.

#### Device profiles

Which physical control carries which `ControlId`, and how state is rendered back,
lives in a **device profile** — data, not code, keyed by USB/MIDI identity.
Profiles ship for known hardware and are learnable for unknown hardware through
the same wizard the guitar uses. See [CONTROLLER_MAPPING.md](CONTROLLER_MAPPING.md)
for the two profiles that exist today and for what is verified versus assumed.

---

## 4. Parameter system and modulation matrix

This is the unification that keeps Phase-2 §3, §4, §11 and §18 from becoming
three parallel systems.

```
effective(param) = clamp( base(param) + Σ modulation(param), range )
```

* Every automatable value has a `ParamId` and a `ParamDesc` in a central
  registry: range, taper (linear/exponential), unit, display name, **and the
  plain-language help text** required by §18. The tooltip and the value formatter
  read from the same place, so they cannot drift apart.
* `base` is what a GUI knob shows and what a preset stores.
* Modulation sources — macros, LFOs, envelopes, mapped continuous controls —
  all write into the same matrix. "Whammy → cutoff" is therefore not a feature,
  it is a row.
* Values feeding DSP are **smoothed** (one-pole, ~5 ms) so a jumping MIDI CC or a
  yanked whammy does not produce zipper noise.

### Macros (§11)

A macro is a named modulation source with several destinations:

```rust
struct Macro { id: MacroId, name: &'static str, targets: Vec<(ParamId, f32 /*depth*/, Curve)> }
```

`BRIGHT · DARK · WET · DRY · ENERGY · CHAOS · DENSITY · SPACE`. `CHAOS` raising
pattern variation, LFO amount, FX modulation and rhythmic variation at once is
simply four target rows.

A macro may be **unipolar** (`0..1`) or **bipolar** (`-1..+1`). The brief's names
come in opposing pairs — bright/dark, wet/dry, calm/chaotic, simple/complex — and
those are one axis each, with the displayed name following the sign. This is not
a reinterpretation for its own sake: endless encoders have no end stop, so a
centre-detented axis is the control they are physically built for. All eight
names survive; four of them are the negative half of an axis. Because the looper is event-based (§7), turning a macro
also re-voices material you already recorded — that is the intended "this is not
normal music software" moment.

---

## 5. Audio graph — two zones

The single most consequential decision in FLUX, and the one with the biggest
failure mode if taken naively.

A freely patchable graph is trivial while monophonic. It becomes a research
project the moment sixteen simultaneous notes must pass through a user-patched
filter: you have to decide what exists per voice and what exists once. Real
modular hardware sidesteps this by being monophonic. FLUX cannot.

FLUX therefore uses the model real polyphonic synthesisers use — **two zones**:

```
VOICE ZONE — compiled once, executed per active voice
┌──────────────────────────────────────────────┐
│  OSC ──► FILTER ──► VCA                      │
│   ▲        ▲          ▲                      │
│  ENV     LFO(v)      ENV                     │
└──────────────────┬───────────────────────────┘
                   │ AUDIO (summed over voices)
                   ▼
GLOBAL ZONE — executed once per block
┌──────────────────────────────────────────────┐
│  MIXER ──► DISTORTION ──► DELAY ──► REVERB ──► OUTPUT
│    ▲                                         │
│  DRUM   SEQUENCER   LFO(g)   SAMPLER         │
└──────────────────────────────────────────────┘
```

Edits inside the voice zone apply to all voices simultaneously. The zone
boundary is one-way: voice → global. This keeps `OSCILLATOR`, `FILTER`,
`ENVELOPE` and `LFO` genuine first-class modules as §5 requires, while
polyphony stays a property of the engine rather than of the patch.

### Modules

```rust
trait Module {
    fn spec(&self) -> &ModuleSpec;             // ports + parameters, static
    fn prepare(&mut self, ctx: &PrepareCtx);   // sample rate, max block — allocation happens HERE
    fn process(&mut self, io: &mut ProcessCtx); // realtime, allocation-free
    fn reset(&mut self);
}
```

`prepare` is the only place a module may allocate. `process` receives
pre-assigned buffer slices and never sees the graph.

Initial set: `OSCILLATOR · FILTER · ENVELOPE · LFO · VCA · SEQUENCER · DRUM ·
SAMPLER · MIXER · DELAY · REVERB · DISTORTION · OUTPUT`.

### Signal types and connection validity (§6)

| from ↓ / to → | AUDIO | CONTROL | GATE | TRIGGER | CLOCK |
|---|---|---|---|---|---|
| **AUDIO**   | ✓ | ✓ (audio-rate mod) | ✗ | ✗ | ✗ |
| **CONTROL** | ✓ (as signal) | ✓ | ✗ | ✗ | ✗ |
| **GATE**    | ✗ | ✓ (0/1) | ✓ | ✓ (rising edge) | ✗ |
| **TRIGGER** | ✗ | ✗ | ✗ | ✓ | ✗ |
| **CLOCK**   | ✗ | ✗ | ✗ | ✓ | ✓ |

`CONTROL → GATE` is deliberately invalid: turning a continuous value into a gate
requires a threshold, which is a decision the user must make explicitly (a future
`COMPARATOR` module) rather than have guessed. `TRIGGER → GATE` is invalid
because a trigger carries no duration. The matrix lives in one table in
`graph/signal.rs` and is unit-tested in both directions.

### Scheduling

Patch editing happens on the GUI thread. On every change the patch is validated
(port types, cycle detection), topologically sorted, and compiled into a
`Schedule`: a flat list of steps plus buffer indices from a pre-allocated pool.
The finished `Schedule` is handed to the audio thread by pointer swap; the
outgoing one is dropped **on the GUI thread**. The audio thread therefore never
allocates, never frees and never sorts.

Graph cycles are rejected at edit time with a readable message. Intentional
feedback (delay, reverb) lives *inside* stateful modules, not as a graph cycle —
this is the standard resolution and it keeps the sort total.

### Denormals

Filters and reverb tails decay into denormal floats, which are catastrophically
slow on some CPUs. Every recursive module squashes its state through a
`flush_denormal` helper. This is cheap and easy to forget, so it is a documented
rule and a smoke test, not a habit.

---

## 6. Synth voice

Sixteen voices in a fixed pool, no allocation, last-note-priority stealing with
a fast release on the stolen voice (an abrupt cut clicks).

Per voice: two oscillators with detune, a TPT state-variable filter (stable at
high resonance, unlike a naive digital ladder), ADSR, velocity.

Oscillators use **PolyBLEP** band-limiting for saw and square. A naive ramp
aliases audibly and is the classic reason a hand-written synth sounds cheap.
This costs roughly twenty lines of arithmetic and is the difference between the
first keypress being convincing or not — which is the actual success criterion
of milestone 1.

---

## 7. Looper — event-based, not audio-based

Two designs are possible. FLUX records **note events with sample-accurate
timestamps** and replays them into the synth, rather than recording rendered
audio.

Reasons, all from the brief:

* §15 requires timing to be corrected after the fact. Impossible with audio.
* §14 requires `undo overdub`. With events this is truncating a length field.
* §16 (Phase 2 §11) requires macro changes to be immediately audible. With an
  event looper, changing a preset or a macro **re-voices material already
  recorded** — the loop follows the patch.

The cost is honest and stated: external audio cannot be looped. When microphone
input arrives, audio tracks are added as a second track kind alongside event
tracks. The two coexist; neither replaces the other.

```rust
struct LoopEvent { pos_in_loop: u32 /*samples*/, action: LoopAction } // Copy
struct LoopTrack {
    events: [LoopEvent; 2048],   // fixed, no allocation on the audio thread
    len: usize,
    len_before_pass: usize,      // single-level overdub undo
    state: TrackState,           // Empty | Recording | Playing | Muted
}
```

Insertion keeps the array sorted by position. Worst case is a `memmove` of the
whole array — 2048 entries of 16 bytes, so ~32 KB, which is a few microseconds
and acceptable inside a callback. Playback advances a cursor per block
and splits the block when the loop wraps, so an event never lands one block late.

### Transport and loop length

The transport always runs on a bar grid derived from BPM. Recording arms and
**starts at the next bar**; the second press ends it and the length is rounded up
to whole bars, minimum one. The first recorded track defines the master loop
length; every later track fills exactly that. Synchronisation is therefore
structural — the user cannot record something that does not fit.

A **click** ships in M1. Recording into silence with no reference is not a
usable first experience.

### Quantization (§15)

```
quantize(pos, grid) = round(pos / grid) * grid,  grid ∈ {1/4, 1/8, 1/16, 1/32, off}
```
Default `1/16`. If the quantized position lands slightly in the future, the note
first *sounds* on the next loop pass — the live note has already been heard in
real time. This is standard looper behaviour and is asserted in a test so the
edge case cannot silently regress.

---

## 8. Views

Two top-level views, switchable without a mouse (§8):

**PERFORMANCE** — big controls, loop lanes with position, BPM, key/scale,
current scene, active pattern, macro knobs, live input feedback.

**PATCH** — modules as cards, animated connections, signal activity, visible
modulation. In M1 the patch is fixed and this view is **read-only**: it shows the
real running graph with live levels. That satisfies "the user can see what is
happening" early, without pulling cable-editing complexity forward.

---

## 9. Live safety (§16)

* The audio thread has no reference to USB or MIDI. A controller disconnect
  cannot reach it — playback continues by construction, not by handling.
* Destructive actions (`Clear Loop`, `Reset Patch`) require a confirmation or a
  held modifier.
* Scene changes can be quantized to the bar.
* BPM changes ramp rather than jump.
* A disconnect raises a non-modal notice that names what still works:
  *"Guitar disconnected. Your loops are still running."*

---

## 10. Diagnostics (§19)

Hidden view: audio latency (ms and frames), sample rate, buffer size, DSP load,
active voices, active loops, MIDI events/s, HID events/s, **dropped events**,
**audio underruns**. All fed from the atomics and counters described in §2.

---

## 11. Module structure

```
src/
├── main.rs
├── app.rs                    eframe App, wiring, view switching
├── core/                     PURE. no audio, no GUI, no I/O — the tested heart
│   ├── event.rs              ControlId, ControlValue, ControlEvent, Action, EngineEvent
│   ├── ids.rs                ParamId, ModuleId, PortId, TrackId, MacroId, SceneId
│   ├── music.rs              notes, scales, keys, note→frequency, scale snapping
│   ├── transport.rs          BPM ↔ samples, bars/beats, position arithmetic
│   ├── quantize.rs           grid quantization
│   └── time.rs               host-time ↔ sample-position mapping
├── params/
│   ├── registry.rs           ParamId → range, taper, unit, help text
│   ├── smoothing.rs          one-pole smoothers
│   ├── modmatrix.rs          base + Σ modulation
│   └── macros.rs             macro definitions → matrix rows
├── graph/
│   ├── signal.rs             SignalType + compatibility matrix
│   ├── module.rs             Module trait, ModuleSpec, PortSpec
│   ├── patch.rs              nodes, edges, validation, cycle detection
│   ├── schedule.rs           topological sort → Schedule, buffer allocation
│   └── modules/              osc filter env lfo vca mixer delay reverb
│                             distortion drum sampler seq output
├── audio/
│   ├── engine.rs             owns patch, voices, looper, transport
│   ├── voice.rs              allocator, per-voice state
│   ├── looper.rs             event tracks
│   ├── click.rs              metronome
│   └── dsp.rs                polyblep, svf, adsr, noise, denormal guard
├── engine/
│   ├── host.rs               CPAL devices, stream setup
│   ├── command.rs            AudioCommand
│   └── telemetry.rs          atomics + counters
├── input/
│   ├── source.rs             trait InputSource, trait ControlSurface, SurfaceState
│   ├── keyboard.rs           computer keyboard (no feedback)
│   ├── xinput.rs             rusb reader thread (Xbox 360 / X-plorer)
│   ├── midi.rs               midir in + out, port discovery
│   ├── surfaces/
│   │   ├── launchkey_mk4.rs  DAW handshake, pad LEDs, encoders, OLED, scale push
│   │   └── generic_midi.rs   class-compliant fallback, no feedback
│   ├── profile.rs            device profiles as data, keyed by USB/MIDI identity
│   ├── mapping.rs            ControlId → Binding, ArcSwap snapshot
│   └── learn.rs
├── ui/
│   ├── theme.rs              tokens: colour, spacing, type, motion
│   ├── widgets/              knob, meter, loop_lane, module_card, cable
│   ├── performance.rs  patch.rs  settings.rs  help.rs  onboarding.rs  debug.rs
└── config/
    ├── settings.rs           audio device, buffer, theme
    └── preset.rs             serde: patch, params, macros, mapping
```

One binary crate, strict module tree — not a Cargo workspace. The boundaries are
drawn so that a later split is mechanical; adding workspace ceremony now buys
nothing (brief §36).

---

## 12. Testing

The pure layer carries the tests, because it carries the behaviour:

* `resolve(mapping, held, event) → actions` — every interaction rule
* quantization, including the "lands in the future" edge case
* loop timing, including wrap across a block boundary
* transport arithmetic (BPM ↔ samples, bar rounding)
* scales, keys, note→frequency
* signal-type compatibility, in both directions
* cycle detection and topological order
* preset round-trip (serialize → deserialize → identical)
* queue behaviour under overflow, including that the drop counter increments

DSP gets smoke tests: render N blocks, assert no NaN/Inf, output within range,
ADSR shape monotone in the expected segments, filter stable at maximum resonance.

**Every new assertion is mutation-checked**: break the thing on purpose, watch
the test go red, restore. A test that has never been seen to fail is not a
guarantee.

---

## 13. Dependencies

| crate | why | why not something else |
|---|---|---|
| `eframe` / `egui` | immediate-mode GUI, trivial to drive at 60 fps with live meters | retained-mode toolkits fight per-frame audio visualisation |
| `cpal` | cross-platform audio I/O, gives us the callback directly | `rodio` owns playback and hides the callback we need |
| `crossbeam-queue` | `ArrayQueue`: bounded, lock-free, allocation-free push/pop | channels allocate or block |
| `crossbeam-channel` | non-realtime GUI-side messaging | — |
| `arc-swap` | wait-free mapping snapshot for input threads | `RwLock` in an input hot path risks priority inversion |
| `rusb` (`vendored`) | the guitar is **not** HID; raw USB is the only route on macOS | `hidapi`/`gilrs` cannot see this device at all — see §14 |
| `midir` | cross-platform MIDI in **and out** — output drives pad LEDs, the OLED and clock | — |
| `serde` + `serde_json` | presets, settings, mappings | — |
| `directories` | correct config paths per OS | — |
| `thiserror` / `anyhow` | typed errors in libraries, context at the edges | — |
| `log` + `env_logger` | diagnostics | — |

Deliberately absent for now: `symphonia` (no file decoding until the sampler
loads user files), `rodio` (we own the callback).

---

## 14. The guitar: measured, not assumed

The connected device was inspected before any code was designed around it:

```
Guitar Hero X-plorer     VID 0x1430 (RedOctane Inc(c)2006)   PID 0x4748
serial 075E517           bDeviceClass 255 (vendor-specific)
Interface 0   0xFF / 0x5D / 0x01   2 endpoints    ← XInput, this is the one
Interface 1   0xFF / 0x5D / 0x03   4 endpoints    (headset)
Interface 2   0xFF / 0x5D / 0x02   1 endpoint
Interface 3   0xFF / 0xFD / 0x13   0 endpoints    (security)
```

It appears under `IOUSB` but **not** under `IOHIDDevice`. That is by design, not
a fault: Xbox 360 controllers are vendor-specific, not HID. **Any HID crate will
never see this device on macOS.** The only route is raw USB on interface 0,
which is why `rusb` is a load-bearing dependency rather than a convenience.

No kernel driver has claimed the interface (`IOCFPlugInTypes` shows the generic
`IOUSBLib`), so libusb should be able to claim it. "Should" is not "does" — this
is verified by a ~30-line probe as the **first implementation task**, before
anything is built on top of it.

Full protocol notes for this device and for the Launchkey Mini MK4 — including
what is verified and what is still assumed — are in
[CONTROLLER_MAPPING.md](CONTROLLER_MAPPING.md).

Button and axis assignments are **not guessed**. The setup wizard (brief §6)
learns them by diffing report bytes while the user presses each control. This
doubles as insurance against this particular unit deviating from the documented
layout.

---

## 15. Decision log

| # | Decision | Rejected alternative | Why |
|---|---|---|---|
| 1 | Single crate, strict modules | Cargo workspace | Ceremony without benefit; split later is mechanical |
| 2 | Audio thread is the only clock | GUI-driven timing | Sample-accurate loops and quantization require it |
| 3 | Input threads push commands directly | Route all input via GUI | GUI vsync would add up to 16 ms jitter to the guitar |
| 4 | Atomics for continuous telemetry | Everything through the event queue | 200 Hz telemetry would starve real events |
| 5 | **Event-based looper** | Audio looper | Enables post-hoc quantization, cheap undo, and macros re-voicing recorded material |
| 6 | **Two-zone graph** (voice ×16 + global) | Flat patchable graph | Flat is either monophonic or a research project |
| 7 | Cycles rejected; feedback inside modules | Graph feedback with 1-block delay | Keeps the topological sort total and the error message honest |
| 8 | One mod matrix for macros, LFOs, controls | Separate systems per source | Otherwise §3, §11 and §6 become three parallel implementations |
| 9 | Raw USB via `rusb` for the guitar | `hidapi` / `gilrs` | Measured: the device is not exposed as HID on macOS |
| 10 | Fixed patch in M1, read-only patch view | Cable editing in M1 | Shows real signal flow early without the editing complexity |
| 11 | Synthesised drums, no samples | Sample library | No third-party rights, no assets to ship (brief §26) |
| 12 | Loop length in whole bars, first track is master | Free-length loops | Makes desynchronisation structurally impossible |
| 13 | **MIDI output is load-bearing** (reversal) | MIDI input only | Was justified with "nothing consumes it yet". The Launchkey Mini MK4 does: pads, screen and encoder positions are all output. Reversed the same day it was written |
| 14 | `ControlSurface` trait on top of `InputSource` | One-directional input only | Modern controllers carry state; a source that cannot render it back leaves the hardware half dead |
| 15 | Encoders driven in **relative** mode | Absolute + host-pushed position | Switching macro pages would otherwise jump values. Relative is structurally jump-free; the screen carries the readout instead |
| 16 | Macros may be bipolar | Eight independent unipolar macros | The brief's names are opposing pairs, and endless encoders have no end stop — a centred axis is what the hardware is for |
| 17 | FLUX owns key and scale, and pushes them to the device | Let the controller's scale mode run independently | One source of truth; also makes §15 musical safety physical — out-of-key keys stop responding on the hardware |
