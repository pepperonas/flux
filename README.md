# FLUX

**Version 0.1.0** — M1b foundation

**FLUX is not a DAW. FLUX is a playable music instrument.**

A live music engine you play with a computer keyboard, a Guitar Hero controller
and MIDI hardware — not with a mouse and a timeline. Synthesis, drums,
generative patterns, live looping and a virtual modular signal graph, arranged
so that the first sixty seconds are interesting even if you have never used
music software before.

> Status: **early development.** The architecture is written down and agreed;
> M1b foundation is implemented on the `m1b-looper` branch. See
> [ROADMAP.md](docs/ROADMAP.md) and [CHANGELOG.md](CHANGELOG.md) for the exact
> scope and remaining hardware work.

Audio outputs can be refreshed and selected from Settings. MIDI ports are
polled automatically and reconnect when devices are plugged in or removed;
Diagnostics shows the current MIDI and Xplorer connection state. The Xplorer
reader is reconnect-safe as well, while its control-byte mapping remains a
hardware verification step.

The current package version is `0.1.0`; run `cargo run -- --version` to print
it without opening the GUI.

---

## Why it exists

Music software mostly asks you to *operate* it. You arrange, you configure, you
click. FLUX asks you to *play* it. Everything that matters during a performance
is reachable from an instrument you hold: frets select, the strum bar fires,
the whammy bends a filter, tilt opens a reverb. The mouse stays available for
setup and editing — it is simply not on the critical path.

The design consequence that follows: **every input is the same kind of input.**
A fret, a MIDI knob and a computer key all become the same control event, and the
audio engine cannot tell them apart. That is what makes "learn mode" three lines
instead of three implementations, and what keeps the system open to hardware
nobody has thought of yet.

## What it does

* **Synthesis** — 16 voices, band-limited oscillators, resonant filter, ADSR, LFOs
* **Live looping** — multi-track, bar-synchronised, quantized, with overdub undo.
  Loops store *notes*, not audio, so changing a sound changes what you already
  recorded
* **Virtual modular** — oscillators, filters, envelopes, LFOs, sequencers, drums
  and effects as real modules with typed ports; invalid patches are refused with
  a reason, not a crash
* **Macros** — `BRIGHT` `DARK` `WET` `DRY` `ENERGY` `CHAOS` `DENSITY` `SPACE`
  move many parameters at once, so you steer the music instead of adjusting it
* **Controllers** — computer keyboard, Novation Launchkey Mini MK4 (RGB pads
  showing loop state, encoders on the macros, parameter names on its screen),
  Guitar Hero / Xbox 360 guitar over raw USB, and generic MIDI. Anything is
  mappable to anything, and mappings are made by moving the control
* **Generative patterns** — constrained by scale, key and density, so variation
  stays musical

## Design principles

1. The audio thread never blocks — no allocation, no locks, no I/O.
2. All input goes through one control layer. The engine never sees a key, a fret
   or a CC number.
3. One mechanism beats three special cases: macros, LFOs and mapped knobs are all
   rows in the same modulation matrix.
4. The audio thread is the only clock.
5. Understandable beats clever.
6. Nothing you are playing may be destroyed by a disconnect or a mis-click.

## Building

Requires a recent stable Rust toolchain.

```bash
git clone https://github.com/pepperonas/flux.git
cd flux
cargo run --release
```

`libusb` is compiled in (`rusb` with the `vendored` feature), so no system
package is needed for controller support.

Platforms: macOS and Windows are targets; Linux follows. Development happens on
macOS (Apple Silicon).

### Controllers

Xbox 360 guitars are **not** HID devices — they use a vendor-specific interface
(`0xFF/0x5D/0x01`). No HID library can see them on macOS. FLUX talks to
interface 0 over raw USB directly, so no driver or kernel extension is required.
Button and axis positions are *learned* through the setup wizard rather than
assumed, so unusual units work too.

Class-compliant MIDI devices need no setup at all. Known controllers get a
profile that drives their lights and screens; unknown ones fall back to the same
learning wizard.

## Development

```bash
cargo check     # fast feedback
cargo test      # the pure layer carries the suite
cargo run       # play it
```

The logic worth testing — control mapping, quantization, loop timing, transport
arithmetic, signal-type validity, preset round-trips — lives in modules with no
audio and no GUI dependency, so the suite runs fast and says something real.
New assertions are mutation-checked: break the behaviour on purpose, watch the
test fail, restore it. A test that has never been seen to fail is not a guarantee.

## Documentation

| Document | Contents |
|---|---|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Threads, control layer, modulation matrix, audio graph, looper, decision log |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Milestones and what "done" means for each |
| [docs/PHASE2.md](docs/PHASE2.md) | Why the control abstraction was built in rather than retrofitted |
| [docs/CONTROLLER_MAPPING.md](docs/CONTROLLER_MAPPING.md) | Device protocols and default mappings, marked verified / documented / assumed |

## Licence

MIT © Martin Pfeffer — [celox.io](https://celox.io)
