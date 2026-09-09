# FLUX — Phase 2 analysis

The Phase-2 brief opens with an instruction: *analyse the existing FLUX
prototype first, preserve working features, and change the architecture only
where the new requirements demand it.*

## Finding: there is no prior code

`/Users/martin/claude/FLUX` contained an empty `temp/` directory and no git
repository. No Rust project, no audio engine, no prototype. Phase 1 had reached
the design-approval gate and stopped there deliberately; nothing had been
written.

The instruction is therefore satisfied trivially — there are no working features
to preserve and nothing to refactor. But the finding is worth recording rather
than skipping, because it changes what the right move is.

## Consequence: fold Phase 2 in, do not retrofit it

The expensive part of Phase 2 would have been threading a control abstraction
underneath an engine in which the keyboard already talked to the synth directly.
That is a rewrite of every input path, and it is exactly what the brief's §20
warns against:

```
INPUTS → CONTROL EVENTS → MUSICAL ENGINE → AUDIO ENGINE      always
Guitar → Synth,  Keyboard → Looper,  MIDI → Filter           never
```

Since no such shortcut exists yet, the correct decision is to build the Phase-2
architecture from the first line and skip the migration entirely. The initial
architecture in [ARCHITECTURE.md](ARCHITECTURE.md) is the Phase-2 architecture.

## Where each Phase-2 requirement lives

| § | Requirement | Where it is handled |
|---|---|---|
| 2 | Input abstraction layer | ARCHITECTURE §3 — `ControlId` / `ControlValue` / `ControlEvent`; the engine has no type in scope naming a key, fret or CC |
| 3 | Control sources, runtime mapping | ARCHITECTURE §3 (`Mapping`, `Binding`) + §4 (mod matrix) |
| 4 | Discrete vs. continuous | Encoded in `ControlValue`, not in a convention |
| 5 | Virtual modular system | ARCHITECTURE §5 — `Module` trait, ports, parameters |
| 6 | Signal connections and validity | ARCHITECTURE §5 — signal-type matrix, cycle detection |
| 7 | Patch view | ARCHITECTURE §8 — read-only in M1, editable in M4 |
| 8 | Performance vs. Patch | ARCHITECTURE §8 — two top-level views from M1 |
| 9–10 | Guitar as an instrument, interaction model | `InputChord`; ROADMAP M3 |
| 11 | Macro system | ARCHITECTURE §4 — macros are modulation rows, not a parallel system |
| 12 | MIDI controllers | ROADMAP M2, bidirectional — see [CONTROLLER_MAPPING.md](CONTROLLER_MAPPING.md) |
| 13–14 | Mapping UI, learn mode | ARCHITECTURE §3 — learn is "remember the next `ControlId`", identical for every source |
| 15 | Musical safety | ROADMAP M5 — constrained generation, advanced mode widens |
| 16 | Live safety | ARCHITECTURE §9 — the audio thread cannot see USB or MIDI, so a disconnect cannot reach it |
| 17 | Visual language | ARCHITECTURE §8, ROADMAP M7 |
| 18 | Contextual help | ARCHITECTURE §4 — help text lives in the parameter registry beside range and unit, so tooltip and value cannot drift |
| 19 | Diagnostics | ARCHITECTURE §10 — counters exist from the first commit; they cannot be retrofitted into a path that already dropped silently |
| 20 | Architecture principle | ARCHITECTURE §1.2, enforced by module boundaries |

## Two decisions that needed a human

Both were put to the user and both are now fixed. They are recorded here because
they are the ones that cannot be cheaply reversed later.

**The modular model.** A flat, fully patchable graph is trivial while
monophonic and becomes a research project the moment sixteen voices must pass
through a user-patched filter. Chosen: **two zones** — a voice zone instantiated
per note (OSC, FILTER, ENV, per-voice LFO) feeding a single global zone (DRUM,
SEQ, FX, MIXER, OUT). This keeps `OSCILLATOR`, `FILTER` and `ENVELOPE` genuine
modules as §5 requires while polyphony stays a property of the engine.

**Milestone ordering.** §21 lists the control stack before the modular model and
does not mention the looper at all, because it assumes the looper already exists.
It does not. Chosen: **sound first** — M1 delivers the brief's §39 milestone
(keyboard → synth → loop) on top of the full Phase-2 control and graph
infrastructure. Mapping and learn mode arrive in M2, once there is something
worth mapping to.

## Revision: the Launchkey Mini MK4

After this analysis was written, a Novation Launchkey Mini 25 MK4 was ordered.
It is not merely another MIDI keyboard, and it invalidated a decision made the
same day.

The roadmap had deferred MIDI output with the justification *"nothing consumes it
yet"*. This device does: 16 RGB pads, a host-writable OLED and encoders that must
be told where they point are all output. Without it the hardware is half dead.
MIDI output moved from deferred to load-bearing.

The architectural gap it exposed is more interesting than the reversal. The
`InputSource` trait is one-directional — device to FLUX. A device that carries
state needs the return path, so surfaces gained a second trait
(`ControlSurface`: connect / render / disconnect) and devices gained **profiles**
as data rather than code. That generalises beyond this purchase: a Launchpad, an
APC or future hardware are the same shape, and the guitar is the degenerate case
whose `render` does nothing.

Two consequences were worth having:

* The device's pad flashing is **hardware-synchronised to MIDI beat clock**. If
  FLUX sends clock, a recording pad blinks in time with the music and no timing
  code is written for it.
* The host can set the keyboard's **scale tonic and type**. FLUX owns key and
  scale and pushes them down, so notes outside the key stop responding on the
  hardware itself. Requirement §15, musical safety, becomes physical rather than
  advisory — which software alone cannot achieve.

Milestone order was unchanged: M1 still depends on no external hardware.

## Implementation plan

The order below is the Phase-2 priority list (§21) reconciled with the fact that
nothing exists yet. `cargo check`, `cargo test` and `cargo run` must pass after
every numbered step.

1. **USB probe** — ~30 lines: can libusb claim interface 0 of the X-plorer on
   this Mac? Everything about the guitar rests on the answer, so it is verified
   before anything is built on it, not after. Throwaway code.
2. Cargo project, window, dark theme, view shell
3. `core/` — events, ids, music theory, transport, quantization *(pure, tested)*
4. `params/` — registry, smoothing, modulation matrix, macros *(pure, tested)*
5. `graph/` — module trait, signal matrix, patch validation, scheduling *(pure, tested)*
6. `audio/dsp.rs` — PolyBLEP, TPT filter, ADSR, denormal guard *(tested)*
7. `engine/` — CPAL host, command queue, telemetry → **first sound**
8. `input/keyboard.rs` + mapping → **playable**
9. `audio/looper.rs` + transport + click → **§39 milestone reached**
10. `ui/performance.rs` — loop lanes, macros, meters, note feedback
11. `ui/patch.rs` read-only, `ui/debug.rs`, `ui/help.rs`
12. M1 review, then M2 (MIDI, mapping UI, learn, macros) per the roadmap

Steps 3–6 are pure and carry the test suite. Step 1 is deliberately first and
deliberately disposable.
