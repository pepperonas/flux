# FLUX M1a — Foundation and First Sound — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Press a key on the computer keyboard and hear a convincing polyphonic synth, with the played note visible in the interface.

**Architecture:** A realtime audio thread owns all DSP state and is the only clock. Input threads translate hardware into device-agnostic `ControlEvent`s, resolve them against a mapping snapshot, and push `Copy`-only commands into a lock-free queue. Parameters resolve through one modulation matrix shared by macros, LFOs and mapped controls. The global audio chain runs as a scheduled module graph; the polyphonic voice chain is a fixed hand-written chain in M1a with published module specs, so the patch view can already draw it.

**Tech Stack:** Rust 2021, `eframe`/`egui`, `cpal`, `crossbeam-queue`, `arc-swap`, `serde`, `thiserror`, `log`/`env_logger`. `rusb` appears only in the throwaway probe in Task 1.

**Spec:** [`docs/ARCHITECTURE.md`](../../ARCHITECTURE.md), with [`docs/ROADMAP.md`](../../ROADMAP.md) (milestone M1) and [`docs/CONTROLLER_MAPPING.md`](../../CONTROLLER_MAPPING.md).

**Follow-on:** M1b (transport, click, event looper, performance view) completes milestone M1 and is planned separately once this plan lands.

## Global Constraints

- **The audio callback never allocates, locks, blocks, or frees.** No `Vec::push` that can grow, no `String`, no `HashMap`, no channel that blocks, no file I/O, no `println!`. Anything needing heap is prepared on another thread and handed over.
- **`src/core/` is pure.** It must not depend on `eframe`, `egui`, `cpal`, or any I/O crate. Adding such a dependency to `core` is a plan violation. This is what keeps the test suite fast and honest.
- **All `AudioCommand` variants are `Copy`.** If a command needs owned data, that is a design error to raise, not to work around.
- **Every new assertion is mutation-checked.** Break the behaviour on purpose, watch the test go red, restore it. A test never seen failing is not a guarantee. Record it in the commit message when a mutation revealed a weak assertion.
- **Rust edition 2021**, toolchain 1.98 or newer (verified present on the development machine).
- **UI text is English.** `BRIGHT`, `CHAOS`, `PERFORMANCE`. Code, comments and commit messages are English.
- **Middle C is MIDI note 60 and is called C4.** Octave 4, semitone 0 → note 60. Used consistently by keyboard mapping, note-to-frequency and the interface.
- **Sample format is `f32`, interleaved stereo** at the device's native rate.
- After every task: `cargo check`, `cargo test`, `cargo clippy -- -D warnings`, and `cargo run` must all succeed.

---

## Deliberately not in this plan

Milestone M1 in the roadmap lists these; they belong to **M1b**, which follows
immediately. They are named here so that "M1a is done" cannot be mistaken for
"M1 is done":

| Item | Why it waits |
|---|---|
| Transport, click, event looper | The whole subject of M1b; `core::transport` and `core::quantize` are built here so M1b starts with them tested |
| Loop lanes in the performance view | Nothing to show until the looper exists |
| Patch view, read-only | Needs the running graph, which this plan produces; drawing it is M1b's first task |
| Audio device **selection** in Settings | `AudioHost::devices()` and the preferred-device argument exist; the interface for choosing is M1b |
| `ControlSurface` trait, MIDI, guitar | M2 and M3. The seam they enter through — `ControlEvent` and `resolve()` — is built here |
| Macros beyond BRIGHT, WET and CHAOS | Defined in full, but ENERGY, DENSITY, SPACE, TIGHT and SIMPLE have no routes until patterns exist in M5. The interface marks them inactive rather than offering a knob that moves nothing |

---

## File Structure

Files created by this plan, and what each is responsible for. Files listed for M1b are not created here.

| File | Responsibility |
|---|---|
| `Cargo.toml` | Dependencies, release profile |
| `src/main.rs` | Entry point, logging, eframe launch |
| `src/app.rs` | `FluxApp`: owns non-realtime state, wires input to engine, switches views |
| `src/core/mod.rs` | Re-exports; asserts purity by having no I/O dependency |
| `src/core/ids.rs` | `ParamId`, `ModuleId`, `TrackId`, `MacroId`, `ModSourceId`, `DeviceId`, `MidiPortId` |
| `src/core/music.rs` | `Scale`, `Key`, `note_to_freq`, `note_for`, scale snapping |
| `src/core/transport.rs` | `Transport`: BPM ↔ samples, bars, boundaries. Used in M1a for the sample clock only |
| `src/core/quantize.rs` | `Grid`, `quantize`, `quantize_in_loop` (written here, consumed by M1b) |
| `src/core/event.rs` | `ControlId`, `ControlValue`, `ControlEvent`, `Action`, `KeyCode` |
| `src/input/mod.rs` | Module wiring |
| `src/input/mapping.rs` | `Binding`, `Mapping`, `HeldSet`, `PlayState`, `resolve()` — the tested heart |
| `src/input/keyboard.rs` | egui key events → `ControlEvent`; default keyboard mapping |
| `src/params/mod.rs` | Module wiring |
| `src/params/registry.rs` | `ParamDesc`, `ParamRegistry`, normalise/denormalise, help text |
| `src/params/smoothing.rs` | `Smoother`: one-pole, zipper-free parameter changes |
| `src/params/modmatrix.rs` | `ModMatrix`: base + Σ routes, allocation-free recompute |
| `src/params/macros.rs` | The eight macros and their routes; unipolar and bipolar |
| `src/audio/mod.rs` | Module wiring |
| `src/audio/dsp.rs` | `flush_denormal`, `poly_blep`, `Osc`, `Svf`, `Adsr` |
| `src/audio/voice.rs` | `Voice` (fixed voice chain), `VoicePool` (allocation and stealing) |
| `src/audio/engine.rs` | `AudioEngine`: drains commands, runs voices then the global graph |
| `src/graph/mod.rs` | Module wiring |
| `src/graph/signal.rs` | `SignalType`, `can_connect` |
| `src/graph/module.rs` | `Module` trait, `ModuleSpec`, `PortSpec`, `ProcessCtx`, `Zone` |
| `src/graph/patch.rs` | `Patch`: nodes, edges, validation, cycle detection |
| `src/graph/schedule.rs` | `Schedule`, `BufferPool`, topological compilation, execution |
| `src/graph/modules/mod.rs` | Registry of module specs, including voice-zone specs |
| `src/graph/modules/mixer.rs` | Global: sums voice output, applies master gain |
| `src/graph/modules/delay.rs` | Global: stereo delay with feedback |
| `src/graph/modules/reverb.rs` | Global: simple feedback-delay-network reverb |
| `src/graph/modules/output.rs` | Global: terminal node, writes the device buffer |
| `src/graph/modules/lfo.rs` | Global: modulation source feeding the mod matrix |
| `src/engine/mod.rs` | Module wiring |
| `src/engine/command.rs` | `AudioCommand` (all `Copy`), `EngineEvent` |
| `src/engine/telemetry.rs` | Atomics and drop counters shared with the interface |
| `src/engine/host.rs` | CPAL device enumeration, stream construction, latency reporting |
| `src/ui/mod.rs` | Module wiring, `View` enum |
| `src/ui/theme.rs` | Colour, spacing, type and motion tokens; egui `Visuals` |
| `src/ui/performance.rs` | M1a: keyboard readout, active notes, macro knobs, meter |
| `src/ui/debug.rs` | Diagnostics: latency, load, voices, dropped events, underruns |

---

## Task 1: USB claim probe — throwaway

This answers one question before anything depends on the answer: **can libusb claim interface 0 of the Guitar Hero X-plorer on this Mac?** Everything about milestone M3 rests on it. It produces no code that is kept.

Measured facts about the device (from `docs/CONTROLLER_MAPPING.md`): VID `0x1430`, PID `0x4748`, interface 0 is `0xFF/0x5D/0x01` with 2 endpoints, and no kernel driver has matched it.

**Files:**
- Create: `<scratchpad>/usbprobe/Cargo.toml` — **outside the repository**, in the session scratchpad
- Create: `<scratchpad>/usbprobe/src/main.rs`

**Interfaces:**
- Consumes: nothing
- Produces: a written finding recorded in `docs/CONTROLLER_MAPPING.md`. No code.

- [ ] **Step 1: Create the probe project outside the repo**

```bash
mkdir -p "$SCRATCH/usbprobe/src" && cd "$SCRATCH/usbprobe"
cat > Cargo.toml <<'EOF'
[package]
name = "usbprobe"
version = "0.0.0"
edition = "2021"

[dependencies]
rusb = { version = "0.9", features = ["vendored"] }
EOF
```

- [ ] **Step 2: Write the probe**

```rust
// src/main.rs
const VID: u16 = 0x1430;
const PID: u16 = 0x4748;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = rusb::Context::new()?;
    let Some(device) = ctx.devices()?.iter().find(|d| {
        d.device_descriptor()
            .map(|desc| desc.vendor_id() == VID && desc.product_id() == PID)
            .unwrap_or(false)
    }) else {
        println!("NOT FOUND: no {VID:#06x}:{PID:#06x} on the bus. Is the guitar plugged in?");
        return Ok(());
    };

    println!("found device on bus {} address {}", device.bus_number(), device.address());

    // Interface 0 is the XInput interface: class 0xFF, subclass 0x5D, protocol 0x01.
    let config = device.config_descriptor(0)?;
    let iface = config
        .interfaces()
        .flat_map(|i| i.descriptors().collect::<Vec<_>>())
        .find(|d| d.class_code() == 0xFF && d.sub_class_code() == 0x5D && d.protocol_code() == 0x01)
        .ok_or("no 0xFF/0x5D/0x01 interface descriptor")?;

    let iface_num = iface.interface_number();
    let endpoint = iface
        .endpoint_descriptors()
        .find(|e| e.direction() == rusb::Direction::In)
        .ok_or("no IN endpoint")?;
    let addr = endpoint.address();
    println!("interface {iface_num}, IN endpoint {addr:#04x}, max packet {}", endpoint.max_packet_size());

    let mut handle = device.open()?;
    println!("open: OK");

    match handle.kernel_driver_active(iface_num) {
        Ok(active) => println!("kernel driver active: {active}"),
        Err(e) => println!("kernel_driver_active not supported here: {e}"),
    }

    handle.claim_interface(iface_num)?;
    println!("CLAIM: OK  <-- this is the question this probe exists to answer");

    println!("reading for 5 s — press frets, strum, move the whammy…");
    let mut buf = [0u8; 32];
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut last = [0u8; 32];
    while std::time::Instant::now() < deadline {
        match handle.read_interrupt(addr, &mut buf, std::time::Duration::from_millis(200)) {
            Ok(n) if buf[..n] != last[..n] => {
                println!("{:02x?}", &buf[..n]);
                last[..n].copy_from_slice(&buf[..n]);
            }
            Ok(_) => {}
            Err(rusb::Error::Timeout) => {}
            Err(e) => { println!("read error: {e}"); break; }
        }
    }

    handle.release_interface(iface_num)?;
    println!("released. done.");
    Ok(())
}
```

- [ ] **Step 3: Run it with the guitar connected**

Run: `cd "$SCRATCH/usbprobe" && cargo run`

Expected on success: `open: OK`, then `CLAIM: OK`, then changing 20-byte packets while controls are pressed.

Expected failure modes and what each means:
- `NOT FOUND` — the guitar is unplugged. Plug it in and rerun; this is not a result.
- `Access denied (insufficient permissions)` on `open` or `claim` — macOS is refusing. Record it; M3 needs a different approach and the roadmap must say so.
- `Resource busy` — something else claimed the interface. Identify it with `ioreg -p IOUSB -l -w 0`.

- [ ] **Step 4: Record the finding in the repository**

Update the two ❓ entries in `docs/CONTROLLER_MAPPING.md` under "Guitar Hero X-plorer". If the claim succeeded, change the libusb-claim entry from ❓ to ✅ and add the observed IN endpoint address and packet length. If any control's bytes were identified from the dump, note them as ✅ observed — but do **not** remove the learning wizard from the plan; it exists for units that deviate.

If the claim **failed**, write the exact error into the document, and add a line to `docs/ROADMAP.md` under M3 stating that raw USB is unavailable on this machine and the guitar approach needs revisiting. Do not proceed to invent a workaround here — that is a separate design decision.

- [ ] **Step 5: Commit the finding, not the code**

```bash
cd <repo-root>
git add docs/CONTROLLER_MAPPING.md docs/ROADMAP.md
git commit -m "docs: record USB claim probe result for the X-plorer

Throwaway probe run before anything depends on the answer. The probe
project lives in the session scratchpad and is deliberately not committed."
```

The probe directory stays in the scratchpad. It is not added to the repository.

---

## Task 2: Project skeleton — a window that opens

**Files:**
- Create: `Cargo.toml`, `src/main.rs`, `src/app.rs`, `src/ui/mod.rs`, `src/ui/theme.rs`
- Create: `rust-toolchain.toml`

**Interfaces:**
- Consumes: nothing
- Produces: `FluxApp` (implements `eframe::App`), `ui::View` enum with `Performance` and `Debug`, `ui::theme::apply(&egui::Context)`.

- [ ] **Step 1: Write the manifest and toolchain pin**

```toml
# Cargo.toml
[package]
name = "flux"
version = "0.1.0"
edition = "2021"
description = "A playable live music instrument"
license = "MIT"

[dependencies]
eframe = { version = "0.29", default-features = false, features = ["default_fonts", "glow", "wayland", "x11"] }
egui = "0.29"
cpal = "0.15"
crossbeam-queue = "0.3"
arc-swap = "1"
serde = { version = "1", features = ["derive"] }
thiserror = "1"
log = "0.4"
env_logger = "0.11"

[profile.release]
opt-level = 3
lto = "thin"
debug = 1

# Audio must never be slow in a debug build, or the app is untestable while developing.
[profile.dev.package."*"]
opt-level = 2
```

```toml
# rust-toolchain.toml
[toolchain]
channel = "stable"
components = ["clippy", "rustfmt"]
```

- [ ] **Step 2: Write the theme tokens**

```rust
// src/ui/theme.rs
use egui::{Color32, Rounding, Stroke, Visuals};

pub const BG: Color32 = Color32::from_rgb(0x0B, 0x0C, 0x10);
pub const SURFACE: Color32 = Color32::from_rgb(0x14, 0x16, 0x1C);
pub const SURFACE_HI: Color32 = Color32::from_rgb(0x1E, 0x21, 0x2A);
pub const TEXT: Color32 = Color32::from_rgb(0xE6, 0xE8, 0xEF);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x9A, 0x9F, 0xAF);
pub const ACCENT: Color32 = Color32::from_rgb(0x7C, 0xE0, 0xD6);
pub const ACCENT_WARM: Color32 = Color32::from_rgb(0xFF, 0xB3, 0x6B);
pub const DANGER: Color32 = Color32::from_rgb(0xFF, 0x6B, 0x7A);

pub const R_SM: f32 = 6.0;
pub const R_MD: f32 = 12.0;

pub fn apply(ctx: &egui::Context) {
    let mut v = Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = SURFACE;
    v.extreme_bg_color = BG;
    v.override_text_color = Some(TEXT);
    v.widgets.noninteractive.bg_fill = SURFACE;
    v.widgets.inactive.bg_fill = SURFACE_HI;
    v.widgets.hovered.bg_fill = SURFACE_HI;
    v.widgets.active.bg_fill = ACCENT;
    v.widgets.noninteractive.rounding = Rounding::same(R_MD);
    v.widgets.inactive.rounding = Rounding::same(R_MD);
    v.widgets.hovered.rounding = Rounding::same(R_MD);
    v.widgets.active.rounding = Rounding::same(R_MD);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, SURFACE_HI);
    ctx.set_visuals(v);
}
```

- [ ] **Step 3: Write the view shell and app**

```rust
// src/ui/mod.rs
pub mod theme;
pub mod performance;
pub mod debug;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum View {
    #[default]
    Performance,
    Debug,
}

impl View {
    pub fn label(self) -> &'static str {
        match self {
            View::Performance => "PERFORMANCE",
            View::Debug => "DIAGNOSTICS",
        }
    }
}
```

```rust
// src/ui/performance.rs
pub fn show(ui: &mut egui::Ui) {
    ui.heading("FLUX");
    ui.label("Press a key.");
}
```

```rust
// src/ui/debug.rs
pub fn show(ui: &mut egui::Ui) {
    ui.heading("DIAGNOSTICS");
}
```

```rust
// src/app.rs
use crate::ui::{self, View};

pub struct FluxApp {
    view: View,
}

impl Default for FluxApp {
    fn default() -> Self {
        Self { view: View::default() }
    }
}

impl eframe::App for FluxApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // An instrument must redraw continuously: meters and note feedback are live.
        ctx.request_repaint();

        egui::TopBottomPanel::top("nav").show(ctx, |ui| {
            ui.horizontal(|ui| {
                for view in [View::Performance, View::Debug] {
                    if ui.selectable_label(self.view == view, view.label()).clicked() {
                        self.view = view;
                    }
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.view {
            View::Performance => ui::performance::show(ui),
            View::Debug => ui::debug::show(ui),
        });
    }
}
```

```rust
// src/main.rs
mod app;
mod ui;

fn main() -> eframe::Result<()> {
    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("FLUX"),
        ..Default::default()
    };
    eframe::run_native(
        "FLUX",
        options,
        Box::new(|cc| {
            ui::theme::apply(&cc.egui_ctx);
            Ok(Box::new(app::FluxApp::default()))
        }),
    )
}
```

- [ ] **Step 4: Verify it builds and runs**

Run: `cargo check && cargo clippy -- -D warnings && cargo run`
Expected: a dark window titled FLUX with two switchable tabs. Close it.

If `eframe 0.29` has moved the `run_native` creator signature (it returns `Result<Box<dyn App>, _>` in this version), adjust the closure to match the version actually resolved — check `cargo doc --open -p eframe` rather than guessing.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock rust-toolchain.toml src/
git commit -m "feat: project skeleton with dark theme and view shell

Continuous repaint from the start: an instrument's meters and note
feedback are live, so a redraw-on-event model would be wrong here."
```

---

## Task 3: Core identifiers and music theory

**Files:**
- Create: `src/core/mod.rs`, `src/core/ids.rs`, `src/core/music.rs`
- Modify: `src/main.rs` (add `mod core;`)

**Interfaces:**
- Consumes: nothing
- Produces:
  - `ParamId(u16)`, `ModuleId(u16)`, `TrackId(u8)`, `MacroId(u8)`, `ModSourceId(u16)`, `DeviceId(u16)`, `MidiPortId(u16)` — all `Copy + Eq + Hash + Debug`
  - `Scale` enum with `degrees(&self) -> &'static [u8]`
  - `Key { root: u8, scale: Scale }` with `snap(&self, note: u8) -> u8`
  - `note_to_freq(note: f32) -> f32`
  - `note_for(octave: i8, semitone: i8) -> u8`

- [ ] **Step 1: Write the failing tests**

```rust
// src/core/music.rs  (tests at the bottom of the file)
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a4_is_440_hz() {
        assert!((note_to_freq(69.0) - 440.0).abs() < 1e-3);
    }

    #[test]
    fn an_octave_down_halves_the_frequency() {
        assert!((note_to_freq(57.0) - 220.0).abs() < 1e-3);
    }

    #[test]
    fn fractional_notes_bend_between_semitones() {
        // Used by pitch bend and the whammy bar later; must not round to an integer note.
        let quarter_tone = note_to_freq(69.5);
        assert!(quarter_tone > 440.0 && quarter_tone < note_to_freq(70.0));
    }

    #[test]
    fn middle_c_is_note_60() {
        assert_eq!(note_for(4, 0), 60);
    }

    #[test]
    fn note_for_clamps_instead_of_wrapping() {
        // A user holding octave-up must never make the synth play a wrapped low note.
        assert_eq!(note_for(20, 11), 127);
        assert_eq!(note_for(-20, 0), 0);
    }

    #[test]
    fn snapping_leaves_in_key_notes_alone() {
        let key = Key { root: 0, scale: Scale::Major }; // C major
        assert_eq!(key.snap(60), 60); // C
        assert_eq!(key.snap(62), 62); // D
        assert_eq!(key.snap(64), 64); // E
    }

    #[test]
    fn snapping_pulls_out_of_key_notes_down_to_the_nearest_degree() {
        let key = Key { root: 0, scale: Scale::Major };
        assert_eq!(key.snap(61), 60); // C# -> C
        assert_eq!(key.snap(66), 65); // F# -> F
    }

    #[test]
    fn snapping_respects_the_root() {
        let key = Key { root: 2, scale: Scale::Major }; // D major has F#
        assert_eq!(key.snap(66), 66); // F# is in key, must be left alone
        assert_eq!(key.snap(65), 64); // F natural is not; snaps down to E
    }

    #[test]
    fn chromatic_snapping_is_the_identity() {
        let key = Key { root: 7, scale: Scale::Chromatic };
        for n in 0..=127u8 {
            assert_eq!(key.snap(n), n);
        }
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib music`
Expected: compile error — `note_to_freq`, `Key`, `Scale` not found.

- [ ] **Step 3: Write the implementation**

```rust
// src/core/ids.rs
macro_rules! id_type {
    ($name:ident, $inner:ty) => {
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
        pub struct $name(pub $inner);
    };
}

id_type!(ParamId, u16);
id_type!(ModuleId, u16);
id_type!(TrackId, u8);
id_type!(MacroId, u8);
id_type!(ModSourceId, u16);
id_type!(DeviceId, u16);
id_type!(MidiPortId, u16);
```

```rust
// src/core/music.rs
/// Semitone offsets from the root for each supported scale.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scale {
    Chromatic,
    Major,
    NaturalMinor,
    HarmonicMinor,
    Dorian,
    Phrygian,
    Mixolydian,
    PentatonicMinor,
    Blues,
}

impl Scale {
    pub fn degrees(self) -> &'static [u8] {
        match self {
            Scale::Chromatic => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            Scale::Major => &[0, 2, 4, 5, 7, 9, 11],
            Scale::NaturalMinor => &[0, 2, 3, 5, 7, 8, 10],
            Scale::HarmonicMinor => &[0, 2, 3, 5, 7, 8, 11],
            Scale::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            Scale::Phrygian => &[0, 1, 3, 5, 7, 8, 10],
            Scale::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
            Scale::PentatonicMinor => &[0, 3, 5, 7, 10],
            Scale::Blues => &[0, 3, 5, 6, 7, 10],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Scale::Chromatic => "Chromatic",
            Scale::Major => "Major",
            Scale::NaturalMinor => "Minor",
            Scale::HarmonicMinor => "Harmonic Minor",
            Scale::Dorian => "Dorian",
            Scale::Phrygian => "Phrygian",
            Scale::Mixolydian => "Mixolydian",
            Scale::PentatonicMinor => "Pentatonic Minor",
            Scale::Blues => "Blues",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Key {
    /// Root pitch class, 0 = C.
    pub root: u8,
    pub scale: Scale,
}

impl Default for Key {
    fn default() -> Self {
        Key { root: 9, scale: Scale::NaturalMinor } // A minor
    }
}

impl Key {
    /// Move a note to the nearest scale degree at or below it.
    ///
    /// Snapping *down* rather than to-nearest is deliberate: a chromatic run
    /// played upwards then stays monotonic, where nearest-neighbour snapping
    /// makes it stutter back and forth.
    pub fn snap(&self, note: u8) -> u8 {
        let degrees = self.scale.degrees();
        let pitch_class = (note as i16 - self.root as i16).rem_euclid(12) as u8;
        let mut lowered = 0u8;
        for offset in 0..12u8 {
            let candidate = pitch_class.wrapping_sub(offset);
            if candidate <= pitch_class && degrees.contains(&candidate) {
                lowered = offset;
                break;
            }
        }
        note.saturating_sub(lowered)
    }
}

/// Equal temperament, A4 = 440 Hz = note 69. Takes `f32` so pitch bend and the
/// whammy bar can express positions between semitones.
pub fn note_to_freq(note: f32) -> f32 {
    440.0 * ((note - 69.0) / 12.0).exp2()
}

/// Middle C is note 60 and is called C4, so octave 4 semitone 0 is 60.
/// Clamps rather than wrapping: holding octave-up must never produce a bass note.
pub fn note_for(octave: i8, semitone: i8) -> u8 {
    let n = 12 * (octave as i32 + 1) + semitone as i32;
    n.clamp(0, 127) as u8
}
```

```rust
// src/core/mod.rs
//! Pure logic: no audio, no interface, no I/O.
//!
//! Nothing in this module may depend on `eframe`, `egui`, `cpal` or any crate
//! that touches the outside world. That constraint is what keeps the test suite
//! fast and what lets every musical rule be asserted directly.

pub mod ids;
pub mod music;
```

Add `mod core;` to `src/main.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib music`
Expected: 8 passing tests.

- [ ] **Step 5: Mutation-check the snapping tests**

Change `note.saturating_sub(lowered)` to `note` and run `cargo test --lib music`.
Expected: `snapping_pulls_out_of_key_notes_down_to_the_nearest_degree` and `snapping_respects_the_root` fail.
Then restore. If either stayed green, the assertion is too weak — strengthen it before continuing.

- [ ] **Step 6: Commit**

```bash
git add src/core src/main.rs
git commit -m "feat(core): identifiers, scales and note-to-frequency

Snapping moves down to the nearest degree rather than to the nearest
neighbour, so an upward chromatic run stays monotonic instead of
stuttering. note_for clamps, so holding octave-up can never wrap into a
bass note. Mutation-checked."
```

---

## Task 4: Transport and quantization

Written now, consumed by the looper in M1b. The sample clock is used in M1a.

**Files:**
- Create: `src/core/transport.rs`, `src/core/quantize.rs`
- Modify: `src/core/mod.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks
- Produces:
  - `Transport { sample_rate: f32, bpm: f32, sample_pos: u64, playing: bool, loop_len: u64 }`
  - `Transport::samples_per_beat(&self) -> f64`, `samples_per_bar(&self) -> f64`, `pos_in_loop(&self) -> u64`, `boundary_at_or_after(&self, from: u64) -> u64`, `advance(&mut self, frames: u64)`
  - `Grid` enum, `Grid::samples(self, samples_per_beat: f64) -> Option<f64>`
  - `quantize(pos: u64, grid: f64) -> u64`, `quantize_in_loop(pos: u64, grid: f64, loop_len: u64) -> u64`

- [ ] **Step 1: Write the failing tests**

```rust
// src/core/transport.rs
#[cfg(test)]
mod tests {
    use super::*;

    fn t() -> Transport {
        Transport { sample_rate: 48_000.0, bpm: 120.0, ..Default::default() }
    }

    #[test]
    fn at_120_bpm_a_beat_is_half_a_second() {
        assert!((t().samples_per_beat() - 24_000.0).abs() < 1e-6);
    }

    #[test]
    fn a_bar_is_four_beats() {
        assert!((t().samples_per_bar() - 96_000.0).abs() < 1e-6);
    }

    #[test]
    fn a_position_exactly_on_a_bar_line_is_already_a_boundary() {
        // Arming exactly on the line must start now, not wait a whole bar.
        assert_eq!(t().boundary_at_or_after(96_000), 96_000);
        assert_eq!(t().boundary_at_or_after(0), 0);
    }

    #[test]
    fn a_position_inside_a_bar_advances_to_the_next_line() {
        assert_eq!(t().boundary_at_or_after(1), 96_000);
        assert_eq!(t().boundary_at_or_after(95_999), 96_000);
    }

    #[test]
    fn position_in_loop_wraps() {
        let mut tr = t();
        tr.loop_len = 96_000;
        tr.sample_pos = 96_010;
        assert_eq!(tr.pos_in_loop(), 10);
    }

    #[test]
    fn position_in_loop_without_a_loop_is_the_raw_position() {
        let mut tr = t();
        tr.loop_len = 0;
        tr.sample_pos = 12_345;
        assert_eq!(tr.pos_in_loop(), 12_345);
    }

    #[test]
    fn a_tempo_that_is_not_a_whole_number_of_samples_still_advances_exactly() {
        // 140 BPM at 44100 is 18900 samples per beat, but 137 is not integral.
        // Position must stay in samples, never accumulate float drift.
        let mut tr = Transport { sample_rate: 44_100.0, bpm: 137.0, ..Default::default() };
        for _ in 0..1000 {
            tr.advance(512);
        }
        assert_eq!(tr.sample_pos, 512_000);
    }
}
```

```rust
// src/core/quantize.rs
#[cfg(test)]
mod tests {
    use super::*;

    const SPB: f64 = 24_000.0; // 120 BPM at 48 kHz

    #[test]
    fn grid_off_yields_no_grid() {
        assert!(Grid::Off.samples(SPB).is_none());
    }

    #[test]
    fn a_sixteenth_is_a_quarter_of_a_beat() {
        assert_eq!(Grid::Sixteenth.samples(SPB), Some(6_000.0));
    }

    #[test]
    fn quantize_snaps_to_the_nearest_line() {
        assert_eq!(quantize(5_000, 6_000.0), 6_000);
        assert_eq!(quantize(2_000, 6_000.0), 0);
        assert_eq!(quantize(9_000, 6_000.0), 12_000); // exactly halfway rounds up
    }

    #[test]
    fn quantize_with_a_zero_grid_is_the_identity() {
        assert_eq!(quantize(1_234, 0.0), 1_234);
    }

    #[test]
    fn a_note_played_just_before_the_loop_end_wraps_to_the_start() {
        // The defining edge case: quantizing forward past the loop end must wrap
        // to zero, not land beyond the loop and never play.
        let loop_len = 48_000;
        assert_eq!(quantize_in_loop(47_500, 6_000.0, loop_len), 0);
    }

    #[test]
    fn quantize_in_loop_leaves_interior_positions_alone() {
        assert_eq!(quantize_in_loop(5_000, 6_000.0, 48_000), 6_000);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib -- transport quantize`
Expected: compile error — `Transport`, `Grid`, `quantize` not found.

- [ ] **Step 3: Write the implementation**

```rust
// src/core/transport.rs
/// The musical clock. The audio thread owns the only instance; the interface
/// reads a copy of its position from an atomic. Position is in samples and is
/// never derived from wall time, so it cannot drift.
#[derive(Clone, Copy, Debug)]
pub struct Transport {
    pub sample_rate: f32,
    pub bpm: f32,
    pub sample_pos: u64,
    pub playing: bool,
    /// Master loop length in samples. Zero means no loop has been recorded yet.
    pub loop_len: u64,
}

impl Default for Transport {
    fn default() -> Self {
        Transport {
            sample_rate: 48_000.0,
            bpm: 120.0,
            sample_pos: 0,
            playing: true,
            loop_len: 0,
        }
    }
}

/// FLUX is 4/4 throughout milestone M1. Other metres are a later design.
pub const BEATS_PER_BAR: u32 = 4;

impl Transport {
    pub fn samples_per_beat(&self) -> f64 {
        60.0 / self.bpm as f64 * self.sample_rate as f64
    }

    pub fn samples_per_bar(&self) -> f64 {
        self.samples_per_beat() * BEATS_PER_BAR as f64
    }

    pub fn pos_in_loop(&self) -> u64 {
        if self.loop_len == 0 {
            self.sample_pos
        } else {
            self.sample_pos % self.loop_len
        }
    }

    /// The first bar line at or after `from`. "At or after" rather than "after"
    /// so that arming exactly on the line starts immediately.
    pub fn boundary_at_or_after(&self, from: u64) -> u64 {
        let spb = self.samples_per_bar();
        let bars = (from as f64 / spb).ceil();
        (bars * spb).round() as u64
    }

    pub fn advance(&mut self, frames: u64) {
        if self.playing {
            self.sample_pos += frames;
        }
    }
}
```

```rust
// src/core/quantize.rs
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Grid {
    Off,
    Quarter,
    Eighth,
    #[default]
    Sixteenth,
    ThirtySecond,
}

impl Grid {
    pub fn samples(self, samples_per_beat: f64) -> Option<f64> {
        let divisor = match self {
            Grid::Off => return None,
            Grid::Quarter => 1.0,
            Grid::Eighth => 2.0,
            Grid::Sixteenth => 4.0,
            Grid::ThirtySecond => 8.0,
        };
        Some(samples_per_beat / divisor)
    }

    pub fn label(self) -> &'static str {
        match self {
            Grid::Off => "OFF",
            Grid::Quarter => "1/4",
            Grid::Eighth => "1/8",
            Grid::Sixteenth => "1/16",
            Grid::ThirtySecond => "1/32",
        }
    }
}

pub fn quantize(pos: u64, grid: f64) -> u64 {
    if grid <= 0.0 {
        return pos;
    }
    ((pos as f64 / grid).round() * grid).round() as u64
}

/// Quantize a position inside a loop.
///
/// A note played just before the loop end snaps *forward* past the end. Wrapping
/// it to the start is what makes it audible on the next pass; without the wrap it
/// would sit beyond the loop and never play again. The note has already been
/// heard live at the moment it was struck, so nothing is lost.
pub fn quantize_in_loop(pos: u64, grid: f64, loop_len: u64) -> u64 {
    let q = quantize(pos, grid);
    if loop_len == 0 {
        q
    } else {
        q % loop_len
    }
}
```

Add `pub mod transport;` and `pub mod quantize;` to `src/core/mod.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib -- transport quantize`
Expected: 13 passing tests.

- [ ] **Step 5: Mutation-check the two load-bearing behaviours**

1. Change `.ceil()` to `.floor() + 1.0` in `boundary_at_or_after`.
   Expected: `a_position_exactly_on_a_bar_line_is_already_a_boundary` fails. Restore.
2. Remove the `% loop_len` from `quantize_in_loop` (return `q`).
   Expected: `a_note_played_just_before_the_loop_end_wraps_to_the_start` fails. Restore.

If either stays green the assertion is not pinning the behaviour. Fix the test, not the mutation.

- [ ] **Step 6: Commit**

```bash
git add src/core
git commit -m "feat(core): transport arithmetic and quantization

Position is in samples and never derived from wall time, so tempi that
are not a whole number of samples per beat cannot drift.

Two edge cases are pinned because they are the ones that silently ruin a
looper: a position exactly on a bar line is already a boundary (arming on
the line must start now), and a note quantized forward past the loop end
wraps to zero instead of landing beyond the loop and never playing.
Both mutation-checked."
```

---

## Task 5: Control events and actions

**Files:**
- Create: `src/core/event.rs`
- Modify: `src/core/mod.rs`

**Interfaces:**
- Consumes: `ParamId`, `MacroId`, `DeviceId`, `MidiPortId` from Task 3
- Produces:
  - `KeyCode` — FLUX's own key enum, deliberately **not** `egui::Key`
  - `ControlValue { Gate(bool), Trigger, Continuous(f32), Delta(i32) }`
  - `GuitarControl`, `MidiControl`, `ControlId`, `ControlEvent`
  - `Action`, `TransportCmd`
  - `ControlValue::as_continuous(self) -> Option<f32>`, `ControlValue::is_press(self) -> bool`

- [ ] **Step 1: Write the failing tests**

```rust
// src/core/event.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ids::MidiPortId;
    use std::collections::HashSet;

    #[test]
    fn control_ids_are_usable_as_map_keys() {
        // Learn mode depends on this: "remember the next ControlId" is only
        // three lines because every source produces the same hashable identity.
        let mut set = HashSet::new();
        set.insert(ControlId::Keyboard(KeyCode::A));
        set.insert(ControlId::Keyboard(KeyCode::A));
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn the_midi_channel_is_part_of_the_identity() {
        // The Launchkey uses the channel as a namespace: pads on 10, encoders
        // on 16, encoder touch on 15. Folding it away would merge controls that
        // are physically different things.
        let pad = ControlId::Midi { port: MidiPortId(0), channel: 10, control: MidiControl::Note(36) };
        let key = ControlId::Midi { port: MidiPortId(0), channel: 1, control: MidiControl::Note(36) };
        assert_ne!(pad, key);
    }

    #[test]
    fn the_same_control_on_two_ports_is_two_controls() {
        let a = ControlId::Midi { port: MidiPortId(0), channel: 1, control: MidiControl::Cc(17) };
        let b = ControlId::Midi { port: MidiPortId(1), channel: 1, control: MidiControl::Cc(17) };
        assert_ne!(a, b);
    }

    #[test]
    fn a_gate_going_down_is_a_press_and_going_up_is_not() {
        assert!(ControlValue::Gate(true).is_press());
        assert!(!ControlValue::Gate(false).is_press());
        assert!(ControlValue::Trigger.is_press());
    }

    #[test]
    fn only_continuous_values_read_as_continuous() {
        assert_eq!(ControlValue::Continuous(0.5).as_continuous(), Some(0.5));
        assert_eq!(ControlValue::Gate(true).as_continuous(), None);
        assert_eq!(ControlValue::Trigger.as_continuous(), None);
        assert_eq!(ControlValue::Delta(3).as_continuous(), None);
    }

    #[test]
    fn every_action_is_copy() {
        // The audio command queue is allocation-free, so nothing reachable from
        // an Action may own heap memory. This test fails to compile if that breaks.
        fn assert_copy<T: Copy>() {}
        assert_copy::<Action>();
        assert_copy::<ControlEvent>();
        assert_copy::<ControlId>();
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib event`
Expected: compile error — `ControlId`, `KeyCode`, `Action` not found.

- [ ] **Step 3: Write the implementation**

```rust
// src/core/event.rs
use crate::core::ids::{DeviceId, MacroId, MidiPortId, ParamId};

/// FLUX's own key identity.
///
/// Deliberately not `egui::Key`: `core` must stay free of interface crates, and
/// the mapping layer is where a windowing key becomes a FLUX control. Values are
/// physical positions, so the layout behaves the same on QWERTZ and QWERTY.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum KeyCode {
    A, B, C, D, E, F, G, H, I, J, K, L, M,
    N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9, Num0,
    Space, Comma, Period, Minus, Plus, Slash, Backslash,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ControlValue {
    /// Held: a key, a fret, a pad.
    Gate(bool),
    /// Momentary impulse with no duration: a strum, a transport button.
    Trigger,
    /// Normalised 0.0..=1.0: a knob, a fader, whammy, tilt, a CC, pad pressure.
    Continuous(f32),
    /// Relative movement from an endless encoder.
    Delta(i32),
}

impl ControlValue {
    pub fn as_continuous(self) -> Option<f32> {
        match self {
            ControlValue::Continuous(v) => Some(v),
            _ => None,
        }
    }

    pub fn is_press(self) -> bool {
        matches!(self, ControlValue::Gate(true) | ControlValue::Trigger)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum GuitarControl {
    Fret(u8),
    StrumUp,
    StrumDown,
    Whammy,
    Tilt,
    Start,
    Select,
    Dpad(u8),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum MidiControl {
    Note(u8),
    Cc(u8),
    PolyAftertouch(u8),
    ChannelPressure,
    PitchBend,
}

/// The identity of one physical control, whatever produced it.
///
/// The audio engine never sees this type. That is the whole point: it cannot
/// tell a fret from a knob from a key, which is what keeps FLUX open to
/// hardware nobody has thought of yet.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ControlId {
    Keyboard(KeyCode),
    Guitar { device: DeviceId, control: GuitarControl },
    Midi { port: MidiPortId, channel: u8, control: MidiControl },
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ControlEvent {
    pub id: ControlId,
    pub value: ControlValue,
    /// Monotonic host time. Milestone M1a stamps commands at drain time; the
    /// field exists now so that turning on sub-block placement later is not a
    /// signature change through five modules.
    pub host_time_ns: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TransportCmd {
    Play,
    Stop,
    Toggle,
}

/// What the musical engine is asked to do. Every variant is `Copy`, because
/// these travel through an allocation-free queue into the audio thread.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    NoteOn { note: u8, velocity: f32 },
    NoteOff { note: u8 },
    SetMacro { macro_id: MacroId, value: f32 },
    SetParam { target: ParamId, value: f32 },
    Transport(TransportCmd),
    OctaveShift(i8),
    VelocityShift(f32),
}
```

Add `pub mod event;` to `src/core/mod.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib event`
Expected: 6 passing tests.

- [ ] **Step 5: Mutation-check the identity tests**

Remove the `channel` field from the `ControlId::Midi` variant and adjust the constructors.
Expected: `the_midi_channel_is_part_of_the_identity` fails to compile or fails.
Restore. Compilation failure counts here: the test is pinning a structural property, and losing it is exactly what we want to be impossible.

- [ ] **Step 6: Commit**

```bash
git add src/core
git commit -m "feat(core): control events and actions

The MIDI channel is part of ControlId, not a detail. On the Launchkey the
channel is the namespace - pads on 10, encoders on 16, touch on 15 - so
folding it away would merge controls that are physically different things.

KeyCode is FLUX's own enum rather than egui::Key so that core stays free
of interface crates. Every Action is Copy, enforced by a test, because
these travel through an allocation-free queue."
```

---

## Task 6: Mapping resolution — the tested heart

This is the single most heavily tested function in FLUX. It is pure, it runs on input threads (never in the audio callback), and every interaction rule in the instrument is a property of it.

**Files:**
- Create: `src/input/mod.rs`, `src/input/mapping.rs`
- Modify: `src/main.rs` (add `mod input;`)

**Interfaces:**
- Consumes: `ControlId`, `ControlEvent`, `ControlValue`, `Action`, `KeyCode` (Task 5); `ParamId`, `MacroId` (Task 3); `note_for` (Task 3)
- Produces:
  - `Curve { Linear, Exponential, SCurve }` with `apply(self, x: f32) -> f32`
  - `Binding { Note { semitone: i8 }, Act(Action), Param { target: ParamId, depth: f32, curve: Curve }, Macro { macro_id: MacroId, curve: Curve } }`
  - `InputChord { held: Vec<ControlId>, trigger: ControlId }`
  - `Mapping { bindings: HashMap<ControlId, Vec<Binding>>, chords: Vec<(InputChord, Binding)> }` with `Mapping::insert`, `Mapping::add_chord`
  - `HeldSet` with `press`, `release`, `contains`, `len`
  - `PlayState { octave: i8, velocity: f32 }`
  - `resolve(&Mapping, &HeldSet, &PlayState, &ControlEvent) -> Vec<Action>`

Allocating a `Vec` here is correct and intentional: `resolve` never runs on the audio thread.

- [ ] **Step 1: Write the failing tests**

```rust
// src/input/mapping.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::event::{ControlEvent, ControlId, ControlValue, KeyCode, MidiControl};
    use crate::core::ids::{MacroId, MidiPortId, ParamId};

    fn ev(id: ControlId, value: ControlValue) -> ControlEvent {
        ControlEvent { id, value, host_time_ns: 0 }
    }

    fn play() -> PlayState {
        PlayState { octave: 4, velocity: 0.8 }
    }

    #[test]
    fn a_bound_key_press_produces_a_note_on_at_the_current_octave() {
        let mut m = Mapping::default();
        m.insert(ControlId::Keyboard(KeyCode::A), Binding::Note { semitone: 0 });
        let out = resolve(&m, &HeldSet::default(), &play(),
                          &ev(ControlId::Keyboard(KeyCode::A), ControlValue::Gate(true)));
        assert_eq!(out, vec![Action::NoteOn { note: 60, velocity: 0.8 }]);
    }

    #[test]
    fn releasing_the_same_key_produces_the_matching_note_off() {
        let mut m = Mapping::default();
        m.insert(ControlId::Keyboard(KeyCode::A), Binding::Note { semitone: 0 });
        let out = resolve(&m, &HeldSet::default(), &play(),
                          &ev(ControlId::Keyboard(KeyCode::A), ControlValue::Gate(false)));
        assert_eq!(out, vec![Action::NoteOff { note: 60 }]);
    }

    #[test]
    fn the_octave_at_press_time_decides_the_note() {
        let mut m = Mapping::default();
        m.insert(ControlId::Keyboard(KeyCode::A), Binding::Note { semitone: 0 });
        let state = PlayState { octave: 2, velocity: 1.0 };
        let out = resolve(&m, &HeldSet::default(), &state,
                          &ev(ControlId::Keyboard(KeyCode::A), ControlValue::Gate(true)));
        assert_eq!(out, vec![Action::NoteOn { note: 36, velocity: 1.0 }]);
    }

    #[test]
    fn an_unbound_control_produces_nothing() {
        let m = Mapping::default();
        let out = resolve(&m, &HeldSet::default(), &play(),
                          &ev(ControlId::Keyboard(KeyCode::Q), ControlValue::Gate(true)));
        assert!(out.is_empty());
    }

    #[test]
    fn a_continuous_control_bound_to_a_macro_sets_it() {
        let mut m = Mapping::default();
        let knob = ControlId::Midi { port: MidiPortId(0), channel: 16, control: MidiControl::Cc(21) };
        m.insert(knob, Binding::Macro { macro_id: MacroId(3), curve: Curve::Linear });
        let out = resolve(&m, &HeldSet::default(), &play(), &ev(knob, ControlValue::Continuous(0.25)));
        assert_eq!(out, vec![Action::SetMacro { macro_id: MacroId(3), value: 0.25 }]);
    }

    #[test]
    fn a_gate_bound_to_a_macro_produces_nothing() {
        // A discrete control cannot drive a continuous target. Silently sending
        // 0.0 or 1.0 would look like a working mapping and behave like a switch.
        let mut m = Mapping::default();
        let key = ControlId::Keyboard(KeyCode::Z);
        m.insert(key, Binding::Macro { macro_id: MacroId(0), curve: Curve::Linear });
        let out = resolve(&m, &HeldSet::default(), &play(), &ev(key, ControlValue::Gate(true)));
        assert!(out.is_empty());
    }

    #[test]
    fn a_param_binding_scales_by_depth() {
        let mut m = Mapping::default();
        let knob = ControlId::Midi { port: MidiPortId(0), channel: 16, control: MidiControl::Cc(22) };
        m.insert(knob, Binding::Param { target: ParamId(7), depth: 0.5, curve: Curve::Linear });
        let out = resolve(&m, &HeldSet::default(), &play(), &ev(knob, ControlValue::Continuous(1.0)));
        assert_eq!(out, vec![Action::SetParam { target: ParamId(7), value: 0.5 }]);
    }

    #[test]
    fn a_chord_fires_only_while_its_modifier_is_held() {
        let mut m = Mapping::default();
        let green = ControlId::Keyboard(KeyCode::F);
        let red = ControlId::Keyboard(KeyCode::G);
        m.insert(red, Binding::Note { semitone: 0 });
        m.add_chord(
            InputChord { held: vec![green], trigger: red },
            Binding::Act(Action::Transport(crate::core::event::TransportCmd::Toggle)),
        );

        // Without the modifier, the plain binding applies.
        let plain = resolve(&m, &HeldSet::default(), &play(), &ev(red, ControlValue::Gate(true)));
        assert_eq!(plain, vec![Action::NoteOn { note: 60, velocity: 0.8 }]);

        // With it held, the chord wins and the plain binding is suppressed.
        let mut held = HeldSet::default();
        held.press(green);
        let chorded = resolve(&m, &held, &play(), &ev(red, ControlValue::Gate(true)));
        assert_eq!(chorded, vec![Action::Transport(crate::core::event::TransportCmd::Toggle)]);
    }

    #[test]
    fn a_chord_needs_every_one_of_its_modifiers() {
        let mut m = Mapping::default();
        let a = ControlId::Keyboard(KeyCode::A);
        let b = ControlId::Keyboard(KeyCode::B);
        let trigger = ControlId::Keyboard(KeyCode::C);
        m.add_chord(
            InputChord { held: vec![a, b], trigger },
            Binding::Act(Action::OctaveShift(1)),
        );

        let mut only_one = HeldSet::default();
        only_one.press(a);
        assert!(resolve(&m, &only_one, &play(), &ev(trigger, ControlValue::Gate(true))).is_empty());

        let mut both = HeldSet::default();
        both.press(a);
        both.press(b);
        assert_eq!(
            resolve(&m, &both, &play(), &ev(trigger, ControlValue::Gate(true))),
            vec![Action::OctaveShift(1)]
        );
    }

    #[test]
    fn a_chord_does_not_fire_on_release() {
        let mut m = Mapping::default();
        let modifier = ControlId::Keyboard(KeyCode::A);
        let trigger = ControlId::Keyboard(KeyCode::C);
        m.add_chord(InputChord { held: vec![modifier], trigger }, Binding::Act(Action::OctaveShift(1)));
        let mut held = HeldSet::default();
        held.press(modifier);
        assert!(resolve(&m, &held, &play(), &ev(trigger, ControlValue::Gate(false))).is_empty());
    }

    #[test]
    fn one_control_may_carry_several_bindings() {
        let mut m = Mapping::default();
        let key = ControlId::Keyboard(KeyCode::A);
        m.insert(key, Binding::Note { semitone: 0 });
        m.insert(key, Binding::Note { semitone: 7 });
        let out = resolve(&m, &HeldSet::default(), &play(), &ev(key, ControlValue::Gate(true)));
        assert_eq!(out.len(), 2);
        assert!(out.contains(&Action::NoteOn { note: 60, velocity: 0.8 }));
        assert!(out.contains(&Action::NoteOn { note: 67, velocity: 0.8 }));
    }

    #[test]
    fn releasing_a_control_that_was_never_held_is_harmless() {
        let mut held = HeldSet::default();
        held.release(ControlId::Keyboard(KeyCode::A));
        assert_eq!(held.len(), 0);
    }

    #[test]
    fn the_exponential_curve_keeps_the_endpoints_and_bends_the_middle() {
        assert!((Curve::Exponential.apply(0.0) - 0.0).abs() < 1e-6);
        assert!((Curve::Exponential.apply(1.0) - 1.0).abs() < 1e-6);
        assert!(Curve::Exponential.apply(0.5) < 0.5);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib mapping`
Expected: compile error — `Mapping`, `resolve`, `HeldSet` not found.

- [ ] **Step 3: Write the implementation**

```rust
// src/input/mapping.rs
use std::collections::{HashMap, HashSet};

use crate::core::event::{Action, ControlEvent, ControlId, ControlValue};
use crate::core::ids::{MacroId, ParamId};
use crate::core::music::note_for;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Curve {
    #[default]
    Linear,
    Exponential,
    SCurve,
}

impl Curve {
    /// Maps 0..=1 to 0..=1. Endpoints are always preserved, so a curve can
    /// never make a control unable to reach its extremes.
    pub fn apply(self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Curve::Linear => x,
            Curve::Exponential => x * x,
            Curve::SCurve => x * x * (3.0 - 2.0 * x),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Binding {
    /// A note relative to the current octave. The octave is applied at resolve
    /// time, not stored, so changing octave affects the next press and never
    /// strands a held note.
    Note { semitone: i8 },
    Act(Action),
    Param { target: ParamId, depth: f32, curve: Curve },
    Macro { macro_id: MacroId, curve: Curve },
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct InputChord {
    pub held: Vec<ControlId>,
    pub trigger: ControlId,
}

#[derive(Clone, Default, Debug)]
pub struct Mapping {
    pub bindings: HashMap<ControlId, Vec<Binding>>,
    pub chords: Vec<(InputChord, Binding)>,
}

impl Mapping {
    pub fn insert(&mut self, id: ControlId, binding: Binding) {
        self.bindings.entry(id).or_default().push(binding);
    }

    pub fn add_chord(&mut self, chord: InputChord, binding: Binding) {
        self.chords.push((chord, binding));
    }
}

/// Which controls are currently held down. Lives on the input thread.
#[derive(Clone, Default, Debug)]
pub struct HeldSet {
    held: HashSet<ControlId>,
}

impl HeldSet {
    pub fn press(&mut self, id: ControlId) {
        self.held.insert(id);
    }

    pub fn release(&mut self, id: ControlId) {
        self.held.remove(&id);
    }

    pub fn contains(&self, id: &ControlId) -> bool {
        self.held.contains(id)
    }

    pub fn len(&self) -> usize {
        self.held.len()
    }

    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }
}

/// Performance state that turns a relative binding into a concrete note.
#[derive(Clone, Copy, Debug)]
pub struct PlayState {
    pub octave: i8,
    pub velocity: f32,
}

impl Default for PlayState {
    fn default() -> Self {
        PlayState { octave: 4, velocity: 0.8 }
    }
}

/// Turn one control event into the actions it means.
///
/// Pure, and never called from the audio callback — which is why returning a
/// `Vec` is fine here. Every interaction rule in FLUX is a property of this
/// function, which is what makes them all assertable.
pub fn resolve(
    mapping: &Mapping,
    held: &HeldSet,
    play: &PlayState,
    event: &ControlEvent,
) -> Vec<Action> {
    // A chord takes precedence over the plain binding of the same control.
    // Without this, pressing a modified combination would fire both meanings.
    if event.value.is_press() {
        for (chord, binding) in &mapping.chords {
            if chord.trigger == event.id && chord.held.iter().all(|id| held.contains(id)) {
                let mut out = Vec::new();
                emit(binding, play, event, &mut out);
                return out;
            }
        }
    }

    let Some(bindings) = mapping.bindings.get(&event.id) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for binding in bindings {
        emit(binding, play, event, &mut out);
    }
    out
}

fn emit(binding: &Binding, play: &PlayState, event: &ControlEvent, out: &mut Vec<Action>) {
    match *binding {
        Binding::Note { semitone } => {
            let note = note_for(play.octave, semitone);
            match event.value {
                ControlValue::Gate(true) | ControlValue::Trigger => {
                    out.push(Action::NoteOn { note, velocity: play.velocity })
                }
                ControlValue::Gate(false) => out.push(Action::NoteOff { note }),
                // A knob bound to a note has no sensible meaning; producing
                // nothing is better than inventing one.
                _ => {}
            }
        }
        Binding::Act(action) => {
            if event.value.is_press() {
                out.push(action);
            }
        }
        Binding::Param { target, depth, curve } => {
            if let Some(v) = event.value.as_continuous() {
                out.push(Action::SetParam { target, value: curve.apply(v) * depth });
            }
        }
        Binding::Macro { macro_id, curve } => {
            if let Some(v) = event.value.as_continuous() {
                out.push(Action::SetMacro { macro_id, value: curve.apply(v) });
            }
        }
    }
}
```

```rust
// src/input/mod.rs
pub mod mapping;
```

Add `mod input;` to `src/main.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib mapping`
Expected: 13 passing tests.

- [ ] **Step 5: Mutation-check the three rules that matter most**

1. Delete the `return out;` inside the chord loop so chords fall through to plain bindings.
   Expected: `a_chord_fires_only_while_its_modifier_is_held` fails.
2. Change `chord.held.iter().all(...)` to `.any(...)`.
   Expected: `a_chord_needs_every_one_of_its_modifiers` fails.
3. In `Binding::Macro`, replace the `as_continuous` guard with `if event.value.is_press() { out.push(Action::SetMacro { macro_id, value: 1.0 }) }`.
   Expected: `a_gate_bound_to_a_macro_produces_nothing` fails.

Restore after each. Every one of these must go red — they are the rules that make chords and control types behave.

- [ ] **Step 6: Commit**

```bash
git add src/input src/main.rs
git commit -m "feat(input): mapping resolution

The tested heart: every interaction rule in FLUX is a property of resolve(),
so every rule is assertable. Pure, and never called from the audio callback -
which is why returning a Vec is correct here rather than a compromise.

Two rules are pinned because getting them wrong is invisible until it
matters: a chord suppresses the plain binding of the same control (otherwise
a modified press fires both meanings), and a discrete control bound to a
continuous target produces nothing rather than a silent 0/1 switch.
All three mutation-checked."
```

---

## Task 7: Parameters, smoothing, modulation matrix, macros

The unification that keeps macros, LFOs and mapped knobs from becoming three parallel systems.

**Files:**
- Create: `src/params/mod.rs`, `src/params/registry.rs`, `src/params/smoothing.rs`, `src/params/modmatrix.rs`, `src/params/macros.rs`
- Modify: `src/main.rs` (add `mod params;`)

**Interfaces:**
- Consumes: `ParamId`, `MacroId`, `ModSourceId` (Task 3)
- Produces:
  - Param constants: `OSC_WAVE`, `OSC_DETUNE`, `OSC_LEVEL`, `FILTER_CUTOFF`, `FILTER_RESONANCE`, `ENV_ATTACK`, `ENV_DECAY`, `ENV_SUSTAIN`, `ENV_RELEASE`, `LFO_RATE`, `LFO_AMOUNT`, `DELAY_TIME`, `DELAY_FEEDBACK`, `DELAY_MIX`, `REVERB_SIZE`, `REVERB_MIX`, `MASTER_GAIN`, and `PARAM_COUNT: usize`
  - `Taper { Linear, Exponential }`, `ParamDesc`, `ParamRegistry` with `desc`, `denormalize`, `normalize`, `format`
  - `Smoother::new(sample_rate, time_ms)`, `set_target`, `snap`, `next`, `current`
  - `ModRoute { source, target, depth }`, `ModMatrix::new(param_count, source_count)`, `set_base`, `base`, `set_source`, `add_route`, `recompute`, `value`
  - `MacroDef`, `MACROS: [MacroDef; 8]`, `MACRO_COUNT`, `macro_source(MacroId) -> ModSourceId`, `MacroDef::label(value) -> &'static str`

- [ ] **Step 1: Write the failing tests**

```rust
// src/params/registry.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_param_id_has_a_description() {
        let reg = ParamRegistry::new();
        for i in 0..PARAM_COUNT {
            let d = reg.desc(ParamId(i as u16));
            assert!(!d.name.is_empty(), "param {i} has no name");
            assert!(!d.help.is_empty(), "param {i} has no help text");
            assert!(d.min < d.max, "param {i} has an empty range");
            assert!(d.default >= d.min && d.default <= d.max, "param {i} default is out of range");
        }
    }

    #[test]
    fn a_linear_param_maps_the_ends_and_the_middle() {
        let reg = ParamRegistry::new();
        assert!((reg.denormalize(FILTER_RESONANCE, 0.0) - 0.5).abs() < 1e-4);
        assert!((reg.denormalize(FILTER_RESONANCE, 1.0) - 20.0).abs() < 1e-4);
        assert!((reg.denormalize(FILTER_RESONANCE, 0.5) - 10.25).abs() < 1e-4);
    }

    #[test]
    fn cutoff_is_exponential_so_the_knob_feels_musical() {
        // A linear cutoff spends most of its travel above 10 kHz, where nothing
        // musically interesting happens. Half-way must land near the geometric
        // mean, not the arithmetic one.
        let reg = ParamRegistry::new();
        assert!((reg.denormalize(FILTER_CUTOFF, 0.0) - 20.0).abs() < 1e-3);
        assert!((reg.denormalize(FILTER_CUTOFF, 1.0) - 20_000.0).abs() < 1.0);
        let mid = reg.denormalize(FILTER_CUTOFF, 0.5);
        assert!((mid - 632.45).abs() < 1.0, "expected the geometric mean, got {mid}");
    }

    #[test]
    fn normalize_and_denormalize_are_inverses() {
        let reg = ParamRegistry::new();
        for id in [FILTER_CUTOFF, FILTER_RESONANCE, ENV_ATTACK, MASTER_GAIN] {
            for n in [0.0f32, 0.13, 0.5, 0.87, 1.0] {
                let round_trip = reg.normalize(id, reg.denormalize(id, n));
                assert!((round_trip - n).abs() < 1e-4, "{id:?} at {n} round-tripped to {round_trip}");
            }
        }
    }
}
```

```rust
// src/params/modmatrix.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ids::{ModSourceId, ParamId};

    fn matrix() -> ModMatrix {
        ModMatrix::new(4, 4)
    }

    #[test]
    fn without_routes_the_value_is_the_base() {
        let mut m = matrix();
        m.set_base(ParamId(0), 0.3);
        m.recompute();
        assert!((m.value(ParamId(0)) - 0.3).abs() < 1e-6);
    }

    #[test]
    fn a_route_adds_source_times_depth() {
        let mut m = matrix();
        m.set_base(ParamId(0), 0.2);
        m.add_route(ModRoute { source: ModSourceId(0), target: ParamId(0), depth: 0.5 });
        m.set_source(ModSourceId(0), 0.6);
        m.recompute();
        assert!((m.value(ParamId(0)) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_negative_source_subtracts() {
        // Bipolar macros depend on this: DARK is BRIGHT with a negative source.
        let mut m = matrix();
        m.set_base(ParamId(0), 0.7);
        m.add_route(ModRoute { source: ModSourceId(0), target: ParamId(0), depth: 0.5 });
        m.set_source(ModSourceId(0), -1.0);
        m.recompute();
        assert!((m.value(ParamId(0)) - 0.2).abs() < 1e-6);
    }

    #[test]
    fn clamping_happens_after_summing_not_per_route() {
        // This is the one that silently ruins modulation. Base 0.5, one route
        // pushing +0.8 and another -0.6 must land on 0.7. Clamping each route as
        // it is applied would give clamp(1.3)=1.0 then 1.0-0.6=0.4 - a different
        // sound, and one that changes depending on route order.
        let mut m = matrix();
        m.set_base(ParamId(0), 0.5);
        m.add_route(ModRoute { source: ModSourceId(0), target: ParamId(0), depth: 1.0 });
        m.add_route(ModRoute { source: ModSourceId(1), target: ParamId(0), depth: 1.0 });
        m.set_source(ModSourceId(0), 0.8);
        m.set_source(ModSourceId(1), -0.6);
        m.recompute();
        assert!((m.value(ParamId(0)) - 0.7).abs() < 1e-6);
    }

    #[test]
    fn the_result_is_clamped_to_the_normalized_range() {
        let mut m = matrix();
        m.set_base(ParamId(0), 0.9);
        m.add_route(ModRoute { source: ModSourceId(0), target: ParamId(0), depth: 1.0 });
        m.set_source(ModSourceId(0), 1.0);
        m.recompute();
        assert!((m.value(ParamId(0)) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn one_source_can_drive_several_targets() {
        // This is what a macro is. Nothing special is needed for it to work.
        let mut m = matrix();
        m.add_route(ModRoute { source: ModSourceId(0), target: ParamId(0), depth: 1.0 });
        m.add_route(ModRoute { source: ModSourceId(0), target: ParamId(1), depth: 0.5 });
        m.set_source(ModSourceId(0), 0.4);
        m.recompute();
        assert!((m.value(ParamId(0)) - 0.4).abs() < 1e-6);
        assert!((m.value(ParamId(1)) - 0.2).abs() < 1e-6);
    }

    #[test]
    fn the_base_survives_recomputation() {
        // recompute() must not write its result back into base, or modulation
        // would ratchet upward on every block.
        let mut m = matrix();
        m.set_base(ParamId(0), 0.5);
        m.add_route(ModRoute { source: ModSourceId(0), target: ParamId(0), depth: 0.3 });
        m.set_source(ModSourceId(0), 1.0);
        for _ in 0..100 {
            m.recompute();
        }
        assert!((m.base(ParamId(0)) - 0.5).abs() < 1e-6);
        assert!((m.value(ParamId(0)) - 0.8).abs() < 1e-6);
    }
}
```

```rust
// src/params/smoothing.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_moves_immediately() {
        let mut s = Smoother::new(48_000.0, 5.0);
        s.snap(0.75);
        assert!((s.next() - 0.75).abs() < 1e-6);
    }

    #[test]
    fn it_approaches_the_target_without_overshooting() {
        let mut s = Smoother::new(48_000.0, 5.0);
        s.snap(0.0);
        s.set_target(1.0);
        let mut previous = 0.0;
        for _ in 0..48_000 {
            let v = s.next();
            assert!(v >= previous - 1e-9, "went backwards: {previous} then {v}");
            assert!(v <= 1.0 + 1e-6, "overshot: {v}");
            previous = v;
        }
        assert!(previous > 0.99, "did not converge, reached only {previous}");
    }

    #[test]
    fn one_time_constant_covers_most_of_the_distance() {
        let mut s = Smoother::new(48_000.0, 5.0);
        s.snap(0.0);
        s.set_target(1.0);
        for _ in 0..240 {
            s.next(); // 5 ms at 48 kHz
        }
        let v = s.current();
        assert!(v > 0.5 && v < 0.75, "expected roughly one time constant, got {v}");
    }
}
```

```rust
// src/params/macros.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_are_eight_macros_for_eight_encoders() {
        assert_eq!(MACROS.len(), 8);
        assert_eq!(MACRO_COUNT, 8);
    }

    #[test]
    fn macro_ids_match_their_position() {
        for (i, m) in MACROS.iter().enumerate() {
            assert_eq!(m.id.0 as usize, i);
        }
    }

    #[test]
    fn every_name_from_the_brief_is_present() {
        // The brief names these explicitly. Losing one to a redesign should
        // break the build, not go unnoticed.
        let names: Vec<&str> = MACROS
            .iter()
            .flat_map(|m| [Some(m.name), m.name_neg].into_iter().flatten())
            .collect();
        for required in ["BRIGHT", "DARK", "WET", "DRY", "ENERGY", "CHAOS", "DENSITY", "SPACE"] {
            assert!(names.contains(&required), "macro name {required} is missing");
        }
    }

    #[test]
    fn a_bipolar_macro_shows_the_other_name_when_negative() {
        let bright = &MACROS[0];
        assert!(bright.bipolar);
        assert_eq!(bright.label(0.6), "BRIGHT");
        assert_eq!(bright.label(-0.6), "DARK");
        assert_eq!(bright.label(0.0), "BRIGHT");
    }

    #[test]
    fn a_unipolar_macro_keeps_its_name() {
        let chaos = MACROS.iter().find(|m| m.name == "CHAOS").unwrap();
        assert!(!chaos.bipolar);
        assert_eq!(chaos.label(0.0), "CHAOS");
        assert_eq!(chaos.label(1.0), "CHAOS");
    }

    #[test]
    fn macro_sources_do_not_collide() {
        use std::collections::HashSet;
        let ids: HashSet<_> = MACROS.iter().map(|m| macro_source(m.id)).collect();
        assert_eq!(ids.len(), MACROS.len());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib -- registry modmatrix smoothing macros`
Expected: compile errors — none of these types exist.

- [ ] **Step 3: Write the implementation**

```rust
// src/params/registry.rs
use crate::core::ids::ParamId;

pub const OSC_WAVE: ParamId = ParamId(0);
pub const OSC_DETUNE: ParamId = ParamId(1);
pub const OSC_LEVEL: ParamId = ParamId(2);
pub const FILTER_CUTOFF: ParamId = ParamId(3);
pub const FILTER_RESONANCE: ParamId = ParamId(4);
pub const ENV_ATTACK: ParamId = ParamId(5);
pub const ENV_DECAY: ParamId = ParamId(6);
pub const ENV_SUSTAIN: ParamId = ParamId(7);
pub const ENV_RELEASE: ParamId = ParamId(8);
pub const LFO_RATE: ParamId = ParamId(9);
pub const LFO_AMOUNT: ParamId = ParamId(10);
pub const DELAY_TIME: ParamId = ParamId(11);
pub const DELAY_FEEDBACK: ParamId = ParamId(12);
pub const DELAY_MIX: ParamId = ParamId(13);
pub const REVERB_SIZE: ParamId = ParamId(14);
pub const REVERB_MIX: ParamId = ParamId(15);
pub const MASTER_GAIN: ParamId = ParamId(16);
pub const PARAM_COUNT: usize = 17;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Taper {
    Linear,
    /// Constant ratio per unit of travel. Required for anything measured in
    /// hertz or seconds, where hearing is logarithmic.
    Exponential,
}

#[derive(Clone, Copy, Debug)]
pub struct ParamDesc {
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub taper: Taper,
    pub unit: &'static str,
    /// Plain language, shown as a tooltip. It lives here rather than in the
    /// interface so that the explanation and the value cannot drift apart.
    pub help: &'static str,
}

pub struct ParamRegistry {
    descs: [ParamDesc; PARAM_COUNT],
}

impl Default for ParamRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ParamRegistry {
    pub fn new() -> Self {
        use Taper::{Exponential as Exp, Linear as Lin};
        let d = |name, min, max, default, taper, unit, help| ParamDesc {
            name, min, max, default, taper, unit, help,
        };
        ParamRegistry {
            descs: [
                d("Waveform", 0.0, 3.0, 2.0, Lin, "",
                  "The basic shape of the sound. Sine is pure and soft, saw is bright and buzzy."),
                d("Detune", 0.0, 50.0, 8.0, Lin, "cents",
                  "Pulls the two oscillators slightly apart. A little makes the sound fatter; a lot makes it seasick."),
                d("Level", 0.0, 1.0, 0.8, Lin, "",
                  "How loud each played note is before the effects."),
                d("Cutoff", 20.0, 20_000.0, 2_000.0, Exp, "Hz",
                  "Frequencies above this are removed. Lower values make the sound darker and rounder."),
                d("Resonance", 0.5, 20.0, 1.0, Lin, "",
                  "Emphasises the frequencies right at the cutoff. Raise it for a sharper, more vocal sound."),
                d("Attack", 0.001, 4.0, 0.005, Exp, "s",
                  "How long the note takes to reach full volume. Short is percussive, long is a swell."),
                d("Decay", 0.001, 4.0, 0.25, Exp, "s",
                  "How long it takes to fall from full volume to the sustain level."),
                d("Sustain", 0.0, 1.0, 0.7, Lin, "",
                  "The level a note holds at while a key stays down."),
                d("Release", 0.001, 8.0, 0.4, Exp, "s",
                  "How long the note takes to fade after the key is let go."),
                d("LFO Rate", 0.01, 20.0, 1.2, Exp, "Hz",
                  "How fast the modulation wobbles."),
                d("LFO Amount", 0.0, 1.0, 0.0, Lin, "",
                  "How far the wobble moves whatever it is connected to."),
                d("Delay Time", 0.01, 2.0, 0.375, Exp, "s",
                  "The gap before each echo repeats."),
                d("Delay Feedback", 0.0, 0.95, 0.35, Lin, "",
                  "How much of each echo is fed back in. High values repeat for a long time."),
                d("Delay Mix", 0.0, 1.0, 0.2, Lin, "",
                  "How much echo is blended into the sound."),
                d("Reverb Size", 0.0, 1.0, 0.5, Lin, "",
                  "How large the imagined room is. Larger sounds more distant."),
                d("Reverb Mix", 0.0, 1.0, 0.18, Lin, "",
                  "How much room is blended into the sound."),
                d("Master", 0.0, 1.0, 0.8, Lin, "",
                  "The overall output level."),
            ],
        }
    }

    pub fn desc(&self, id: ParamId) -> &ParamDesc {
        &self.descs[id.0 as usize]
    }

    /// Normalised 0..1 to the parameter's real units.
    pub fn denormalize(&self, id: ParamId, norm: f32) -> f32 {
        let d = self.desc(id);
        let n = norm.clamp(0.0, 1.0);
        match d.taper {
            Taper::Linear => d.min + n * (d.max - d.min),
            Taper::Exponential => d.min * (d.max / d.min).powf(n),
        }
    }

    /// Real units back to normalised 0..1.
    pub fn normalize(&self, id: ParamId, value: f32) -> f32 {
        let d = self.desc(id);
        let v = value.clamp(d.min, d.max);
        match d.taper {
            Taper::Linear => (v - d.min) / (d.max - d.min),
            Taper::Exponential => (v / d.min).ln() / (d.max / d.min).ln(),
        }
    }

    pub fn format(&self, id: ParamId, norm: f32) -> String {
        let d = self.desc(id);
        let v = self.denormalize(id, norm);
        if d.unit.is_empty() {
            format!("{v:.2}")
        } else if v >= 1000.0 {
            format!("{:.1}k {}", v / 1000.0, d.unit)
        } else {
            format!("{v:.2} {}", d.unit)
        }
    }
}
```

```rust
// src/params/smoothing.rs
/// One-pole smoothing so that a jumping MIDI value or a yanked whammy bar does
/// not produce zipper noise.
#[derive(Clone, Copy, Debug)]
pub struct Smoother {
    current: f32,
    target: f32,
    coeff: f32,
}

impl Smoother {
    pub fn new(sample_rate: f32, time_ms: f32) -> Self {
        let samples = (time_ms / 1000.0 * sample_rate).max(1.0);
        Smoother { current: 0.0, target: 0.0, coeff: 1.0 - (-1.0 / samples).exp() }
    }

    pub fn set_target(&mut self, v: f32) {
        self.target = v;
    }

    /// Jump instantly. Used when a voice starts, where smoothing would be a
    /// slide up from the previous note's value.
    pub fn snap(&mut self, v: f32) {
        self.current = v;
        self.target = v;
    }

    pub fn next(&mut self) -> f32 {
        self.current += (self.target - self.current) * self.coeff;
        self.current
    }

    pub fn current(&self) -> f32 {
        self.current
    }
}
```

```rust
// src/params/modmatrix.rs
use crate::core::ids::{ModSourceId, ParamId};

#[derive(Clone, Copy, Debug)]
pub struct ModRoute {
    pub source: ModSourceId,
    pub target: ParamId,
    pub depth: f32,
}

/// One mechanism for every kind of modulation.
///
/// Macros, LFOs and mapped continuous controls are all just sources with
/// routes. "Whammy to cutoff" is not a feature here, it is a row - which is the
/// whole reason this type exists rather than three separate systems.
pub struct ModMatrix {
    base: Vec<f32>,
    sources: Vec<f32>,
    routes: Vec<ModRoute>,
    out: Vec<f32>,
}

impl ModMatrix {
    pub fn new(param_count: usize, source_count: usize) -> Self {
        ModMatrix {
            base: vec![0.0; param_count],
            sources: vec![0.0; source_count],
            routes: Vec::with_capacity(64),
            out: vec![0.0; param_count],
        }
    }

    pub fn set_base(&mut self, p: ParamId, v: f32) {
        self.base[p.0 as usize] = v.clamp(0.0, 1.0);
    }

    pub fn base(&self, p: ParamId) -> f32 {
        self.base[p.0 as usize]
    }

    pub fn set_source(&mut self, s: ModSourceId, v: f32) {
        self.sources[s.0 as usize] = v.clamp(-1.0, 1.0);
    }

    pub fn source(&self, s: ModSourceId) -> f32 {
        self.sources[s.0 as usize]
    }

    /// Routes are added while the patch is being built, never from the audio
    /// thread - `Vec::push` may allocate.
    pub fn add_route(&mut self, route: ModRoute) {
        self.routes.push(route);
    }

    /// Allocation-free. Runs once per audio block.
    pub fn recompute(&mut self) {
        self.out.copy_from_slice(&self.base);
        for r in &self.routes {
            self.out[r.target.0 as usize] += self.sources[r.source.0 as usize] * r.depth;
        }
        // Clamp only once, after every contribution has been summed. Clamping
        // per route would make the result depend on route order.
        for v in &mut self.out {
            *v = v.clamp(0.0, 1.0);
        }
    }

    pub fn value(&self, p: ParamId) -> f32 {
        self.out[p.0 as usize]
    }

    pub fn values(&self) -> &[f32] {
        &self.out
    }
}
```

```rust
// src/params/macros.rs
use crate::core::ids::{MacroId, ModSourceId};

/// A macro moves several parameters at once, so the player steers the music
/// instead of adjusting it.
///
/// A macro may be bipolar. The brief's names come in opposing pairs, and endless
/// encoders have no end stop, so a centred axis is the control they are
/// physically built for. All eight names survive; four are the negative half of
/// an axis.
#[derive(Clone, Copy, Debug)]
pub struct MacroDef {
    pub id: MacroId,
    pub name: &'static str,
    pub name_neg: Option<&'static str>,
    pub bipolar: bool,
    pub help: &'static str,
}

impl MacroDef {
    pub fn label(&self, value: f32) -> &'static str {
        match self.name_neg {
            Some(neg) if value < 0.0 => neg,
            _ => self.name,
        }
    }
}

pub const MACRO_COUNT: usize = 8;

pub const MACROS: [MacroDef; MACRO_COUNT] = [
    MacroDef { id: MacroId(0), name: "BRIGHT", name_neg: Some("DARK"), bipolar: true,
        help: "Opens or closes the sound. Right is brighter and more present, left is darker and further away." },
    MacroDef { id: MacroId(1), name: "WET", name_neg: Some("DRY"), bipolar: true,
        help: "How much space is around the sound. Right adds echo and room, left brings it close and direct." },
    MacroDef { id: MacroId(2), name: "ENERGY", name_neg: None, bipolar: false,
        help: "How hard the music pushes. Raises level, attack and movement together." },
    MacroDef { id: MacroId(3), name: "CHAOS", name_neg: None, bipolar: false,
        help: "How unpredictable things get. Increases variation in patterns, modulation and effects." },
    MacroDef { id: MacroId(4), name: "DENSITY", name_neg: None, bipolar: false,
        help: "How much is happening. More notes, more hits, more layers." },
    MacroDef { id: MacroId(5), name: "SPACE", name_neg: None, bipolar: false,
        help: "How wide and distant everything sits." },
    MacroDef { id: MacroId(6), name: "TIGHT", name_neg: Some("LOOSE"), bipolar: true,
        help: "How strictly things sit on the beat. Right is locked, left breathes." },
    MacroDef { id: MacroId(7), name: "SIMPLE", name_neg: Some("COMPLEX"), bipolar: true,
        help: "How intricate the material is. Right is elaborate, left is stripped back." },
];

/// Macros occupy the first modulation source slots. LFOs and mapped controls
/// take the ones after them.
pub fn macro_source(id: MacroId) -> ModSourceId {
    ModSourceId(id.0 as u16)
}

pub const LFO_SOURCE_BASE: u16 = MACRO_COUNT as u16;
pub const MOD_SOURCE_COUNT: usize = MACRO_COUNT + 4;
```

```rust
// src/params/mod.rs
pub mod macros;
pub mod modmatrix;
pub mod registry;
pub mod smoothing;
```

Add `mod params;` to `src/main.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib -- registry modmatrix smoothing macros`
Expected: 20 passing tests.

- [ ] **Step 5: Mutation-check the summing rule and the taper**

1. In `recompute`, move the clamp inside the route loop (clamp after each `+=`).
   Expected: `clamping_happens_after_summing_not_per_route` fails. Restore.
2. Change `FILTER_CUTOFF`'s taper to `Linear`.
   Expected: `cutoff_is_exponential_so_the_knob_feels_musical` fails. Restore.
3. In `recompute`, write the result back into `base` as well.
   Expected: `the_base_survives_recomputation` fails. Restore.

- [ ] **Step 6: Commit**

```bash
git add src/params src/main.rs
git commit -m "feat(params): registry, smoothing, modulation matrix, macros

One mechanism for every kind of modulation. Macros, LFOs and mapped knobs
are all sources with routes, so 'whammy to cutoff' is a row rather than a
feature. A macro driving several targets needs no special case.

Clamping happens once after summing, never per route - the per-route
version makes the result depend on route order, which is the kind of bug
that is inaudible until two modulations fight. Pinned and mutation-checked.

Help text lives in the parameter registry beside range and unit, so the
tooltip and the value cannot drift apart."
```

---

## Task 8: DSP primitives

**Files:**
- Create: `src/audio/mod.rs`, `src/audio/dsp.rs`
- Modify: `src/main.rs` (add `mod audio;`)

**Interfaces:**
- Consumes: nothing
- Produces:
  - `flush_denormal(f32) -> f32`
  - `poly_blep(t: f32, dt: f32) -> f32`
  - `Waveform { Sine, Triangle, Saw, Square }` with `from_index(f32) -> Waveform`
  - `Osc::default()`, `Osc::tick(&mut self, freq: f32, sr: f32, wave: Waveform) -> f32`, `Osc::reset`
  - `Svf::default()`, `Svf::lowpass(&mut self, input: f32, cutoff_hz: f32, q: f32, sr: f32) -> f32`, `Svf::reset`
  - `AdsrParams { attack, decay, sustain, release }` (seconds, 0..1 sustain)
  - `Adsr::default()`, `gate_on`, `gate_off`, `tick(&mut self, p: &AdsrParams, sr: f32) -> f32`, `is_idle`

- [ ] **Step 1: Write the failing tests**

```rust
// src/audio/dsp.rs
#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|x| x * x).sum::<f32>() / samples.len() as f32).sqrt()
    }

    #[test]
    fn denormals_are_flushed_but_real_values_survive() {
        assert_eq!(flush_denormal(1e-40), 0.0);
        assert_eq!(flush_denormal(-1e-40), 0.0);
        assert_eq!(flush_denormal(0.5), 0.5);
        assert_eq!(flush_denormal(-1e-6), -1e-6);
    }

    #[test]
    fn poly_blep_is_zero_away_from_the_discontinuity() {
        let dt = 0.01;
        assert_eq!(poly_blep(0.5, dt), 0.0);
        assert_eq!(poly_blep(0.3, dt), 0.0);
    }

    #[test]
    fn poly_blep_is_nonzero_at_the_edges() {
        // This correction is the difference between a synth that sounds cheap
        // and one that does not, so its presence is asserted rather than assumed.
        let dt = 0.01;
        assert!(poly_blep(0.001, dt) != 0.0);
        assert!(poly_blep(0.999, dt) != 0.0);
    }

    #[test]
    fn every_waveform_stays_in_range_across_the_spectrum() {
        for wave in [Waveform::Sine, Waveform::Triangle, Waveform::Saw, Waveform::Square] {
            for freq in [20.0, 440.0, 4_000.0, 12_000.0] {
                let mut osc = Osc::default();
                for _ in 0..4_800 {
                    let v = osc.tick(freq, SR, wave);
                    assert!(v.is_finite(), "{wave:?} at {freq} Hz produced {v}");
                    assert!(v.abs() <= 1.5, "{wave:?} at {freq} Hz produced {v}");
                }
            }
        }
    }

    #[test]
    fn a_sine_has_the_expected_rms() {
        let mut osc = Osc::default();
        let out: Vec<f32> = (0..48_000).map(|_| osc.tick(100.0, SR, Waveform::Sine)).collect();
        assert!((rms(&out) - 0.7071).abs() < 0.01);
    }

    #[test]
    fn band_limiting_reduces_high_frequency_saw_energy() {
        // A naive saw aliases: energy that should not exist folds back down. The
        // band-limited version must have measurably less total energy at a
        // frequency where aliasing is severe.
        let mut naive = 0.0f32;
        let mut naive_out = Vec::new();
        let inc = 6_000.0 / SR;
        for _ in 0..48_000 {
            naive += inc;
            if naive >= 1.0 { naive -= 1.0; }
            naive_out.push(2.0 * naive - 1.0);
        }
        let mut osc = Osc::default();
        let blep_out: Vec<f32> = (0..48_000).map(|_| osc.tick(6_000.0, SR, Waveform::Saw)).collect();
        assert!(rms(&blep_out) < rms(&naive_out),
                "band-limited {} should be below naive {}", rms(&blep_out), rms(&naive_out));
    }

    #[test]
    fn the_filter_passes_a_low_tone_and_removes_a_high_one() {
        let mut low_osc = Osc::default();
        let mut low_filter = Svf::default();
        let low: Vec<f32> = (0..24_000)
            .map(|_| low_filter.lowpass(low_osc.tick(100.0, SR, Waveform::Sine), 2_000.0, 0.707, SR))
            .collect();

        let mut high_osc = Osc::default();
        let mut high_filter = Svf::default();
        let high: Vec<f32> = (0..24_000)
            .map(|_| high_filter.lowpass(high_osc.tick(12_000.0, SR, Waveform::Sine), 2_000.0, 0.707, SR))
            .collect();

        assert!(rms(&low) > 0.6, "the passband was attenuated: {}", rms(&low));
        assert!(rms(&high) < 0.1, "the stopband leaked: {}", rms(&high));
    }

    #[test]
    fn the_filter_stays_stable_at_maximum_resonance() {
        // A naive digital ladder blows up here. This is why the filter is TPT.
        let mut osc = Osc::default();
        let mut filter = Svf::default();
        for _ in 0..48_000 {
            let v = filter.lowpass(osc.tick(220.0, SR, Waveform::Saw), 800.0, 20.0, SR);
            assert!(v.is_finite(), "filter produced {v}");
            assert!(v.abs() < 100.0, "filter ran away to {v}");
        }
    }

    #[test]
    fn the_envelope_rises_holds_and_falls() {
        let p = AdsrParams { attack: 0.01, decay: 0.05, sustain: 0.5, release: 0.05 };
        let mut env = Adsr::default();
        assert!(env.is_idle());

        env.gate_on();
        let mut peak: f32 = 0.0;
        for _ in 0..(0.01 * SR) as usize {
            peak = peak.max(env.tick(&p, SR));
        }
        assert!(peak > 0.95, "attack only reached {peak}");

        for _ in 0..(0.5 * SR) as usize {
            env.tick(&p, SR);
        }
        let held = env.tick(&p, SR);
        assert!((held - 0.5).abs() < 0.02, "sustain settled at {held}");

        env.gate_off();
        for _ in 0..(2.0 * SR) as usize {
            env.tick(&p, SR);
        }
        assert!(env.is_idle(), "envelope never became idle");
        assert_eq!(env.tick(&p, SR), 0.0);
    }

    #[test]
    fn an_idle_envelope_stays_silent_forever() {
        // A voice pool decides a voice is free by asking this. If an idle
        // envelope ever produced signal, freed voices would leak sound.
        let p = AdsrParams { attack: 0.01, decay: 0.05, sustain: 0.5, release: 0.05 };
        let mut env = Adsr::default();
        for _ in 0..1_000 {
            assert_eq!(env.tick(&p, SR), 0.0);
        }
    }

    #[test]
    fn releasing_a_note_that_never_sounded_does_nothing() {
        let p = AdsrParams { attack: 0.01, decay: 0.05, sustain: 0.5, release: 0.05 };
        let mut env = Adsr::default();
        env.gate_off();
        assert!(env.is_idle());
        assert_eq!(env.tick(&p, SR), 0.0);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib dsp`
Expected: compile error — `Osc`, `Svf`, `Adsr` not found.

- [ ] **Step 3: Write the implementation**

```rust
// src/audio/dsp.rs
use std::f32::consts::PI;

/// Filters and reverb tails decay towards zero and end up in denormal floats,
/// which are catastrophically slow on some processors. Every recursive piece of
/// state passes through here. It is cheap and easy to forget, so it is a rule
/// rather than a habit.
#[inline]
pub fn flush_denormal(x: f32) -> f32 {
    if x.abs() < 1e-30 {
        0.0
    } else {
        x
    }
}

/// Polynomial band-limited step.
///
/// A naive saw or square jumps between samples, and that jump contains
/// frequencies above half the sample rate which fold back down as aliasing. This
/// correction rounds the jump over one sample. It is about twenty lines of
/// arithmetic and it is the difference between the first keypress being
/// convincing or sounding cheap.
#[inline]
pub fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        2.0 * t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + 2.0 * t + 1.0
    } else {
        0.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Waveform {
    Sine,
    Triangle,
    #[default]
    Saw,
    Square,
}

impl Waveform {
    pub fn from_index(v: f32) -> Waveform {
        match v.round() as i32 {
            0 => Waveform::Sine,
            1 => Waveform::Triangle,
            3 => Waveform::Square,
            _ => Waveform::Saw,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Waveform::Sine => "SINE",
            Waveform::Triangle => "TRI",
            Waveform::Saw => "SAW",
            Waveform::Square => "SQUARE",
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Osc {
    phase: f32,
}

impl Osc {
    pub fn reset(&mut self) {
        self.phase = 0.0;
    }

    pub fn tick(&mut self, freq: f32, sample_rate: f32, wave: Waveform) -> f32 {
        let dt = (freq / sample_rate).clamp(0.0, 0.49);
        let t = self.phase;
        let out = match wave {
            Waveform::Sine => (t * 2.0 * PI).sin(),
            Waveform::Triangle => 4.0 * (t - 0.5).abs() - 1.0,
            Waveform::Saw => 2.0 * t - 1.0 - poly_blep(t, dt),
            Waveform::Square => {
                let raw = if t < 0.5 { 1.0 } else { -1.0 };
                let mut half = t + 0.5;
                if half >= 1.0 {
                    half -= 1.0;
                }
                raw + poly_blep(t, dt) - poly_blep(half, dt)
            }
        };
        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        out
    }
}

/// Topology-preserving-transform state variable filter.
///
/// Chosen over a naive digital ladder because it stays stable at high
/// resonance, where the ladder blows up. That stability is asserted in a test.
#[derive(Clone, Copy, Debug, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
}

impl Svf {
    pub fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }

    pub fn lowpass(&mut self, input: f32, cutoff_hz: f32, q: f32, sample_rate: f32) -> f32 {
        let cutoff = cutoff_hz.clamp(20.0, sample_rate * 0.45);
        let g = (PI * cutoff / sample_rate).tan();
        let k = 1.0 / q.max(0.5);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;

        let v3 = input - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = flush_denormal(2.0 * v1 - self.ic1);
        self.ic2 = flush_denormal(2.0 * v2 - self.ic2);
        v2
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AdsrParams {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

impl Default for AdsrParams {
    fn default() -> Self {
        AdsrParams { attack: 0.005, decay: 0.25, sustain: 0.7, release: 0.4 }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Stage {
    #[default]
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Linear attack, exponential decay and release.
///
/// The attack is linear because a predictable, fast rise is what makes a note
/// feel immediate. Decay and release are exponential because that is how sound
/// actually dies away.
#[derive(Clone, Copy, Debug, Default)]
pub struct Adsr {
    stage: Stage,
    level: f32,
}

/// Below this the release is over. An exponential curve never reaches zero, and
/// a voice that never finishes releasing is a voice the pool can never reuse.
const IDLE_THRESHOLD: f32 = 1e-4;

impl Adsr {
    pub fn gate_on(&mut self) {
        self.stage = Stage::Attack;
    }

    pub fn gate_off(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    pub fn is_idle(&self) -> bool {
        self.stage == Stage::Idle
    }

    pub fn reset(&mut self) {
        self.stage = Stage::Idle;
        self.level = 0.0;
    }

    pub fn tick(&mut self, p: &AdsrParams, sample_rate: f32) -> f32 {
        let coeff = |seconds: f32| 1.0 - (-1.0 / (seconds.max(0.0005) * sample_rate)).exp();
        match self.stage {
            Stage::Idle => {
                self.level = 0.0;
            }
            Stage::Attack => {
                self.level += 1.0 / (p.attack.max(0.0005) * sample_rate);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level += (p.sustain - self.level) * coeff(p.decay);
                if (self.level - p.sustain).abs() < 1e-3 {
                    self.level = p.sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => {
                self.level = p.sustain;
            }
            Stage::Release => {
                self.level -= self.level * coeff(p.release);
                if self.level < IDLE_THRESHOLD {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }
}
```

```rust
// src/audio/mod.rs
pub mod dsp;
```

Add `mod audio;` to `src/main.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib dsp`
Expected: 11 passing tests.

- [ ] **Step 5: Mutation-check the parts that would silently degrade the sound**

1. In `Waveform::Saw`, remove `- poly_blep(t, dt)`.
   Expected: `band_limiting_reduces_high_frequency_saw_energy` fails. Restore.
2. Remove both `flush_denormal` calls in `Svf::lowpass`.
   Expected: nothing fails — and that is the honest result. Denormal performance is not observable from a correctness test. Restore them anyway; the reason they exist is documented at the function, and a benchmark, not a unit test, is what would catch a regression. Note this outcome in the commit message rather than inventing a test that pretends otherwise.
3. Change `IDLE_THRESHOLD` to `0.0`.
   Expected: `the_envelope_rises_holds_and_falls` fails on `env.is_idle()`. Restore.

- [ ] **Step 6: Commit**

```bash
git add src/audio src/main.rs
git commit -m "feat(audio): band-limited oscillators, TPT filter, ADSR

PolyBLEP is the difference between the first keypress sounding convincing
or cheap - a test asserts that band limiting measurably reduces energy at
6 kHz, where naive aliasing is severe.

The filter is TPT rather than a naive ladder because it stays stable at
maximum resonance; that is asserted rather than assumed.

The envelope has an idle threshold because an exponential release never
reaches zero, and a voice that never finishes releasing is one the pool
can never reuse.

Mutation-checking found an honest limit: removing flush_denormal breaks no
test, because denormal cost is a performance property, not a correctness
one. The calls stay, documented at the site; a benchmark would be the
thing that catches their loss."
```

---

## Task 9: Signal types, module trait, patch validation

**Files:**
- Create: `src/graph/mod.rs`, `src/graph/signal.rs`, `src/graph/module.rs`, `src/graph/patch.rs`
- Modify: `src/main.rs` (add `mod graph;`)

**Interfaces:**
- Consumes: `ModuleId`, `ParamId` (Task 3)
- Produces:
  - `SignalType { Audio, Control, Gate, Trigger, Clock }`, `can_connect(from, to) -> bool`
  - `Zone { Voice, Global }`
  - `PortSpec { name, signal }`, `ModuleSpec { name, zone, inputs, outputs, params }`
  - `MAX_INPUTS: usize = 8`, `MAX_OUTPUTS: usize = 4`
  - `ProcessCtx<'a> { frames, sample_rate, inputs, outputs, params }` with `input(&self, i) -> Option<&[f32]>` and `output(&mut self, i) -> Option<&mut [f32]>`
  - `trait Module: Send { spec, prepare, process, reset }`
  - `ModuleKind` enum, `spec_for(ModuleKind) -> &'static ModuleSpec`
  - `PortRef { module, port }`, `PatchError`, `Patch` with `add`, `connect`, `nodes`, `edges`, `topological_order`

- [ ] **Step 1: Write the failing tests**

```rust
// src/graph/signal.rs
#[cfg(test)]
mod tests {
    use super::*;
    use SignalType::*;

    #[test]
    fn audio_flows_into_audio_and_can_modulate() {
        assert!(can_connect(Audio, Audio));
        assert!(can_connect(Audio, Control)); // audio-rate modulation is legitimate
    }

    #[test]
    fn control_is_a_signal_too() {
        assert!(can_connect(Control, Control));
        assert!(can_connect(Control, Audio));
    }

    #[test]
    fn a_gate_can_become_a_trigger_but_not_the_other_way() {
        // A gate has a rising edge, so it can produce a trigger. A trigger has
        // no duration, so it cannot produce a gate - there would be nothing to
        // decide when the gate closes.
        assert!(can_connect(Gate, Trigger));
        assert!(!can_connect(Trigger, Gate));
    }

    #[test]
    fn a_continuous_value_cannot_become_a_gate() {
        // Turning a continuous value into a gate needs a threshold, and that is
        // a decision the player must make explicitly with a comparator module -
        // not one FLUX guesses on their behalf.
        assert!(!can_connect(Control, Gate));
        assert!(!can_connect(Audio, Gate));
    }

    #[test]
    fn clock_drives_triggers_and_clocks_only() {
        assert!(can_connect(Clock, Trigger));
        assert!(can_connect(Clock, Clock));
        assert!(!can_connect(Clock, Audio));
        assert!(!can_connect(Clock, Control));
    }

    #[test]
    fn audio_is_never_a_clock() {
        assert!(!can_connect(Audio, Clock));
        assert!(!can_connect(Control, Clock));
        assert!(!can_connect(Gate, Clock));
    }
}
```

```rust
// src/graph/patch.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_valid_audio_connection_is_accepted() {
        let mut p = Patch::default();
        let mixer = p.add(ModuleKind::Mixer);
        let out = p.add(ModuleKind::Output);
        assert!(p.connect(PortRef { module: mixer, port: 0 }, PortRef { module: out, port: 0 }).is_ok());
        assert_eq!(p.edges().len(), 1);
    }

    #[test]
    fn a_type_mismatch_is_refused_with_the_types_named() {
        // "Verhindere ungueltige Verbindungen" - and say why, so the message is
        // usable rather than just a refusal.
        let mut p = Patch::default();
        let lfo = p.add(ModuleKind::Lfo);
        let out = p.add(ModuleKind::Output);
        let err = p
            .connect(PortRef { module: lfo, port: 0 }, PortRef { module: out, port: 0 })
            .unwrap_err();
        match err {
            PatchError::TypeMismatch { from, to } => {
                assert_eq!(from, SignalType::Control);
                assert_eq!(to, SignalType::Audio);
            }
            other => panic!("expected a type mismatch, got {other:?}"),
        }
    }

    #[test]
    fn an_unknown_port_is_refused() {
        let mut p = Patch::default();
        let out = p.add(ModuleKind::Output);
        let mixer = p.add(ModuleKind::Mixer);
        assert!(matches!(
            p.connect(PortRef { module: mixer, port: 99 }, PortRef { module: out, port: 0 }),
            Err(PatchError::UnknownPort { .. })
        ));
    }

    #[test]
    fn an_input_takes_only_one_connection() {
        // Summing happens in a mixer, explicitly. Letting two sources land on one
        // input would sum them invisibly, and the patch view would show two
        // cables into a socket that looks like it holds one.
        let mut p = Patch::default();
        let a = p.add(ModuleKind::Mixer);
        let b = p.add(ModuleKind::Mixer);
        let out = p.add(ModuleKind::Output);
        p.connect(PortRef { module: a, port: 0 }, PortRef { module: out, port: 0 }).unwrap();
        assert!(matches!(
            p.connect(PortRef { module: b, port: 0 }, PortRef { module: out, port: 0 }),
            Err(PatchError::InputAlreadyConnected { .. })
        ));
    }

    #[test]
    fn a_cycle_is_refused_at_edit_time() {
        // Rejecting the connection is better than accepting it and discovering
        // at compile time that the graph cannot be ordered. The player finds out
        // while their hand is still on the cable.
        let mut p = Patch::default();
        let a = p.add(ModuleKind::Delay);
        let b = p.add(ModuleKind::Delay);
        p.connect(PortRef { module: a, port: 0 }, PortRef { module: b, port: 0 }).unwrap();
        assert!(matches!(
            p.connect(PortRef { module: b, port: 0 }, PortRef { module: a, port: 0 }),
            Err(PatchError::WouldCycle)
        ));
    }

    #[test]
    fn a_module_may_not_feed_itself() {
        let mut p = Patch::default();
        let d = p.add(ModuleKind::Delay);
        assert!(matches!(
            p.connect(PortRef { module: d, port: 0 }, PortRef { module: d, port: 0 }),
            Err(PatchError::WouldCycle)
        ));
    }

    #[test]
    fn topological_order_puts_producers_before_consumers() {
        let mut p = Patch::default();
        let mixer = p.add(ModuleKind::Mixer);
        let delay = p.add(ModuleKind::Delay);
        let out = p.add(ModuleKind::Output);
        p.connect(PortRef { module: mixer, port: 0 }, PortRef { module: delay, port: 0 }).unwrap();
        p.connect(PortRef { module: delay, port: 0 }, PortRef { module: out, port: 0 }).unwrap();

        let order = p.topological_order().unwrap();
        let pos = |m: ModuleId| order.iter().position(|x| *x == m).unwrap();
        assert!(pos(mixer) < pos(delay));
        assert!(pos(delay) < pos(out));
        assert_eq!(order.len(), 3);
    }

    #[test]
    fn unconnected_modules_still_appear_in_the_order() {
        // An orphan module must still be given a turn: it may be a source whose
        // output nobody has patched yet, and skipping it would freeze its state.
        let mut p = Patch::default();
        let _lonely = p.add(ModuleKind::Lfo);
        let out = p.add(ModuleKind::Output);
        let order = p.topological_order().unwrap();
        assert_eq!(order.len(), 2);
        assert!(order.contains(&out));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib -- signal patch`
Expected: compile error — nothing in `graph` exists.

- [ ] **Step 3: Write the implementation**

```rust
// src/graph/signal.rs
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignalType {
    Audio,
    Control,
    Gate,
    Trigger,
    Clock,
}

impl SignalType {
    pub fn name(self) -> &'static str {
        match self {
            SignalType::Audio => "AUDIO",
            SignalType::Control => "CONTROL",
            SignalType::Gate => "GATE",
            SignalType::Trigger => "TRIGGER",
            SignalType::Clock => "CLOCK",
        }
    }
}

/// Which connections are meaningful.
///
/// The refusals carry more design than the permissions. `Control -> Gate` is
/// invalid because turning a continuous value into a gate needs a threshold,
/// and choosing that threshold belongs to the player, not to a guess. `Trigger
/// -> Gate` is invalid because a trigger has no duration, so nothing could
/// decide when the gate closes.
pub fn can_connect(from: SignalType, to: SignalType) -> bool {
    use SignalType::*;
    matches!(
        (from, to),
        (Audio, Audio)
            | (Audio, Control)
            | (Control, Audio)
            | (Control, Control)
            | (Gate, Gate)
            | (Gate, Control)
            | (Gate, Trigger)
            | (Trigger, Trigger)
            | (Clock, Clock)
            | (Clock, Trigger)
    )
}
```

```rust
// src/graph/module.rs
use crate::core::ids::ParamId;
use crate::graph::signal::SignalType;

pub const MAX_INPUTS: usize = 8;
pub const MAX_OUTPUTS: usize = 4;

/// Which zone a module lives in.
///
/// Voice-zone modules are instantiated per sounding note; global-zone modules
/// run once per block. The boundary is one way: voice output sums into global.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Zone {
    Voice,
    Global,
}

#[derive(Clone, Copy, Debug)]
pub struct PortSpec {
    pub name: &'static str,
    pub signal: SignalType,
}

#[derive(Clone, Copy, Debug)]
pub struct ModuleSpec {
    pub name: &'static str,
    pub zone: Zone,
    pub inputs: &'static [PortSpec],
    pub outputs: &'static [PortSpec],
    pub params: &'static [ParamId],
}

/// What a module sees while processing. It never learns about the graph, the
/// patch, or which buffers it was given.
pub struct ProcessCtx<'a> {
    pub frames: usize,
    pub sample_rate: f32,
    pub inputs: [Option<&'a [f32]>; MAX_INPUTS],
    pub outputs: [Option<&'a mut [f32]>; MAX_OUTPUTS],
    /// Resolved, normalised 0..1, indexed by the module spec's `params` order.
    pub params: &'a [f32],
}

impl<'a> ProcessCtx<'a> {
    /// An unconnected input reads as silence. Modules must handle `None` rather
    /// than assume a cable is present.
    pub fn input(&self, index: usize) -> Option<&[f32]> {
        self.inputs.get(index).copied().flatten()
    }

    pub fn output(&mut self, index: usize) -> Option<&mut [f32]> {
        self.outputs.get_mut(index)?.as_deref_mut()
    }

    pub fn param(&self, index: usize) -> f32 {
        self.params.get(index).copied().unwrap_or(0.0)
    }
}

/// A processing unit in the graph.
///
/// `prepare` is the only place a module may allocate. `process` runs in the
/// audio callback and must not allocate, lock, or block.
pub trait Module: Send {
    fn spec(&self) -> &'static ModuleSpec;
    fn prepare(&mut self, sample_rate: f32, max_block: usize);
    /// Modules **overwrite** their outputs; they never accumulate into them.
    /// Summing is a mixer's job, stated explicitly in the patch.
    fn process(&mut self, ctx: &mut ProcessCtx);
    fn reset(&mut self);
}
```

```rust
// src/graph/patch.rs
use crate::core::ids::ModuleId;
use crate::graph::module::{ModuleSpec, Zone};
use crate::graph::signal::{can_connect, SignalType};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModuleKind {
    // Voice zone: specs only in M1a. The voice chain is executed directly in
    // audio::voice so that milestone M1a does not need per-voice graph
    // machinery; the specs exist so the patch view can already draw the chain,
    // and so M4 can make it editable without redefining the contract.
    Oscillator,
    Filter,
    Envelope,
    Vca,
    // Global zone: executed through the schedule.
    Lfo,
    Mixer,
    Delay,
    Reverb,
    Output,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PortRef {
    pub module: ModuleId,
    pub port: u8,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Edge {
    pub from: PortRef,
    pub to: PortRef,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Node {
    pub id: ModuleId,
    pub kind: ModuleKind,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
pub enum PatchError {
    #[error("no module with id {0:?}")]
    UnknownModule(ModuleId),
    #[error("module {module:?} has no port {port}")]
    UnknownPort { module: ModuleId, port: u8 },
    #[error("cannot connect {from:?} to {to:?}")]
    TypeMismatch { from: SignalType, to: SignalType },
    #[error("that input already has a connection")]
    InputAlreadyConnected { module: ModuleId, port: u8 },
    #[error("that connection would create a loop")]
    WouldCycle,
}

#[derive(Clone, Default, Debug)]
pub struct Patch {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    next_id: u16,
}

impl Patch {
    pub fn add(&mut self, kind: ModuleKind) -> ModuleId {
        let id = ModuleId(self.next_id);
        self.next_id += 1;
        self.nodes.push(Node { id, kind });
        id
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    pub fn kind(&self, id: ModuleId) -> Option<ModuleKind> {
        self.nodes.iter().find(|n| n.id == id).map(|n| n.kind)
    }

    fn spec(&self, id: ModuleId) -> Result<&'static ModuleSpec, PatchError> {
        self.kind(id).map(spec_for).ok_or(PatchError::UnknownModule(id))
    }

    pub fn connect(&mut self, from: PortRef, to: PortRef) -> Result<(), PatchError> {
        let from_spec = self.spec(from.module)?;
        let to_spec = self.spec(to.module)?;

        let out = from_spec
            .outputs
            .get(from.port as usize)
            .ok_or(PatchError::UnknownPort { module: from.module, port: from.port })?;
        let inp = to_spec
            .inputs
            .get(to.port as usize)
            .ok_or(PatchError::UnknownPort { module: to.module, port: to.port })?;

        if !can_connect(out.signal, inp.signal) {
            return Err(PatchError::TypeMismatch { from: out.signal, to: inp.signal });
        }

        if self.edges.iter().any(|e| e.to == to) {
            return Err(PatchError::InputAlreadyConnected { module: to.module, port: to.port });
        }

        // Check for a cycle before committing, so the refusal reaches the player
        // while their hand is still on the cable.
        self.edges.push(Edge { from, to });
        if self.topological_order().is_err() {
            self.edges.pop();
            return Err(PatchError::WouldCycle);
        }
        Ok(())
    }

    pub fn disconnect(&mut self, to: PortRef) {
        self.edges.retain(|e| e.to != to);
    }

    /// Kahn's algorithm. An orphan module still gets a turn - it may be a source
    /// nobody has patched yet, and skipping it would freeze its internal state.
    pub fn topological_order(&self) -> Result<Vec<ModuleId>, PatchError> {
        let mut incoming: Vec<usize> = self
            .nodes
            .iter()
            .map(|n| self.edges.iter().filter(|e| e.to.module == n.id).count())
            .collect();

        let mut ready: Vec<ModuleId> = self
            .nodes
            .iter()
            .zip(&incoming)
            .filter(|(_, c)| **c == 0)
            .map(|(n, _)| n.id)
            .collect();

        let index_of = |id: ModuleId| self.nodes.iter().position(|n| n.id == id);
        let mut order = Vec::with_capacity(self.nodes.len());

        while let Some(id) = ready.pop() {
            order.push(id);
            for edge in self.edges.iter().filter(|e| e.from.module == id) {
                if let Some(i) = index_of(edge.to.module) {
                    incoming[i] -= 1;
                    if incoming[i] == 0 {
                        ready.push(self.nodes[i].id);
                    }
                }
            }
        }

        if order.len() == self.nodes.len() {
            Ok(order)
        } else {
            Err(PatchError::WouldCycle)
        }
    }
}

macro_rules! ports {
    ($($name:literal : $sig:ident),* $(,)?) => {
        &[$(crate::graph::module::PortSpec {
            name: $name,
            signal: crate::graph::signal::SignalType::$sig,
        }),*]
    };
}

pub fn spec_for(kind: ModuleKind) -> &'static ModuleSpec {
    use crate::params::registry as p;
    match kind {
        ModuleKind::Oscillator => &ModuleSpec {
            name: "OSCILLATOR", zone: Zone::Voice,
            inputs: ports!("pitch": Control),
            outputs: ports!("audio": Audio),
            params: &[p::OSC_WAVE, p::OSC_DETUNE, p::OSC_LEVEL],
        },
        ModuleKind::Filter => &ModuleSpec {
            name: "FILTER", zone: Zone::Voice,
            inputs: ports!("audio": Audio, "cutoff": Control),
            outputs: ports!("audio": Audio),
            params: &[p::FILTER_CUTOFF, p::FILTER_RESONANCE],
        },
        ModuleKind::Envelope => &ModuleSpec {
            name: "ENVELOPE", zone: Zone::Voice,
            inputs: ports!("gate": Gate),
            outputs: ports!("level": Control),
            params: &[p::ENV_ATTACK, p::ENV_DECAY, p::ENV_SUSTAIN, p::ENV_RELEASE],
        },
        ModuleKind::Vca => &ModuleSpec {
            name: "VCA", zone: Zone::Voice,
            inputs: ports!("audio": Audio, "level": Control),
            outputs: ports!("audio": Audio),
            params: &[],
        },
        ModuleKind::Lfo => &ModuleSpec {
            name: "LFO", zone: Zone::Global,
            inputs: ports!(),
            outputs: ports!("mod": Control),
            params: &[p::LFO_RATE, p::LFO_AMOUNT],
        },
        ModuleKind::Mixer => &ModuleSpec {
            name: "MIXER", zone: Zone::Global,
            inputs: ports!("voices": Audio, "aux": Audio),
            outputs: ports!("audio": Audio),
            params: &[],
        },
        ModuleKind::Delay => &ModuleSpec {
            name: "DELAY", zone: Zone::Global,
            inputs: ports!("audio": Audio),
            outputs: ports!("audio": Audio),
            params: &[p::DELAY_TIME, p::DELAY_FEEDBACK, p::DELAY_MIX],
        },
        ModuleKind::Reverb => &ModuleSpec {
            name: "REVERB", zone: Zone::Global,
            inputs: ports!("audio": Audio),
            outputs: ports!("audio": Audio),
            params: &[p::REVERB_SIZE, p::REVERB_MIX],
        },
        ModuleKind::Output => &ModuleSpec {
            name: "OUTPUT", zone: Zone::Global,
            inputs: ports!("audio": Audio),
            outputs: ports!(),
            params: &[p::MASTER_GAIN],
        },
    }
}
```

```rust
// src/graph/mod.rs
pub mod module;
pub mod patch;
pub mod signal;
```

Add `mod graph;` to `src/main.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib -- signal patch`
Expected: 14 passing tests.

- [ ] **Step 5: Mutation-check the refusals**

1. Add `(Control, Gate)` to the `can_connect` match.
   Expected: `a_continuous_value_cannot_become_a_gate` fails. Restore.
2. Remove the `self.edges.pop()` and the `WouldCycle` return from `connect`.
   Expected: `a_cycle_is_refused_at_edit_time` and `a_module_may_not_feed_itself` fail. Restore.
3. Remove the `InputAlreadyConnected` check.
   Expected: `an_input_takes_only_one_connection` fails. Restore.

- [ ] **Step 6: Commit**

```bash
git add src/graph src/main.rs
git commit -m "feat(graph): signal types, module trait, patch validation

The refusals carry more design than the permissions. Control cannot become
a gate because that needs a threshold, and choosing it belongs to the
player rather than to a guess; a trigger cannot become a gate because it
has no duration, so nothing could decide when the gate closes. Both pinned.

Cycles are refused at connect time rather than discovered at compile time,
so the player finds out while their hand is still on the cable. An input
accepts one connection: summing is a mixer's job, stated in the patch,
because two cables into one socket would sum invisibly.

Voice-zone module specs exist but are not executed through the graph in
M1a - audio::voice runs that chain directly. The specs are the contract so
the patch view can draw it now and M4 can make it editable later."
```

---

## Task 10: Schedule compilation and buffer pool

**Files:**
- Create: `src/graph/schedule.rs`
- Modify: `src/graph/mod.rs`

**Interfaces:**
- Consumes: `Patch`, `PatchError`, `PortRef`, `spec_for`, `ModuleKind`, `Zone` (Task 9); `MAX_INPUTS`, `MAX_OUTPUTS`, `Module`, `ProcessCtx` (Task 9)
- Produces:
  - `Step { module: ModuleId, module_index: usize, inputs: [Option<usize>; MAX_INPUTS], outputs: [Option<usize>; MAX_OUTPUTS], param_base: usize }`
  - `Schedule { steps: Vec<Step>, buffer_count: usize }`, `Schedule::compile(&Patch) -> Result<Schedule, PatchError>`
  - `BufferPool::new(buffer_count, max_block)`, `BufferPool::buffer(&self, index) -> &[f32]`, `BufferPool::clear`
  - `run(&Schedule, &mut BufferPool, &mut [Box<dyn Module>], frames, sample_rate, params: &[f32])`

- [ ] **Step 1: Write the failing tests**

```rust
// src/graph/schedule.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::module::{Module, ModuleSpec, ProcessCtx};
    use crate::graph::patch::{ModuleKind, Patch, PortRef};

    /// Writes a constant, so the order of execution is visible in the output.
    struct Const(f32);
    impl Module for Const {
        fn spec(&self) -> &'static ModuleSpec { crate::graph::patch::spec_for(ModuleKind::Mixer) }
        fn prepare(&mut self, _: f32, _: usize) {}
        fn process(&mut self, ctx: &mut ProcessCtx) {
            let v = self.0;
            if let Some(out) = ctx.output(0) {
                out.fill(v);
            }
        }
        fn reset(&mut self) {}
    }

    /// Adds one to whatever arrives, so a missing input is detectable.
    struct AddOne;
    impl Module for AddOne {
        fn spec(&self) -> &'static ModuleSpec { crate::graph::patch::spec_for(ModuleKind::Delay) }
        fn prepare(&mut self, _: f32, _: usize) {}
        fn process(&mut self, ctx: &mut ProcessCtx) {
            let input: Vec<f32> = match ctx.input(0) {
                Some(s) => s.to_vec(),
                None => vec![0.0; ctx.frames],
            };
            if let Some(out) = ctx.output(0) {
                for (o, i) in out.iter_mut().zip(input) {
                    *o = i + 1.0;
                }
            }
        }
        fn reset(&mut self) {}
    }

    #[test]
    fn a_chain_executes_in_dependency_order() {
        let mut patch = Patch::default();
        let src = patch.add(ModuleKind::Mixer);
        let add = patch.add(ModuleKind::Delay);
        patch.connect(PortRef { module: src, port: 0 }, PortRef { module: add, port: 0 }).unwrap();

        let schedule = Schedule::compile(&patch).unwrap();
        let mut pool = BufferPool::new(schedule.buffer_count, 64);
        let mut modules: Vec<Box<dyn Module>> = vec![Box::new(Const(2.0)), Box::new(AddOne)];

        run(&schedule, &mut pool, &mut modules, 64, 48_000.0, &[0.0; 32]);

        let add_output = schedule.steps.iter().find(|s| s.module == add).unwrap().outputs[0].unwrap();
        assert_eq!(pool.buffer(add_output)[0], 3.0);
    }

    #[test]
    fn an_unconnected_input_reads_as_silence() {
        // A module must never see stale data from a buffer somebody else used.
        let mut patch = Patch::default();
        let add = patch.add(ModuleKind::Delay);
        let schedule = Schedule::compile(&patch).unwrap();
        let mut pool = BufferPool::new(schedule.buffer_count, 64);
        let mut modules: Vec<Box<dyn Module>> = vec![Box::new(AddOne)];

        run(&schedule, &mut pool, &mut modules, 64, 48_000.0, &[0.0; 32]);

        let out = schedule.steps[0].outputs[0].unwrap();
        assert_eq!(pool.buffer(out)[0], 1.0);
    }

    #[test]
    fn every_output_port_gets_its_own_buffer() {
        let mut patch = Patch::default();
        let a = patch.add(ModuleKind::Mixer);
        let b = patch.add(ModuleKind::Mixer);
        let schedule = Schedule::compile(&patch).unwrap();
        let out_a = schedule.steps.iter().find(|s| s.module == a).unwrap().outputs[0].unwrap();
        let out_b = schedule.steps.iter().find(|s| s.module == b).unwrap().outputs[0].unwrap();
        assert_ne!(out_a, out_b);
    }

    #[test]
    fn a_consumers_input_points_at_its_producers_output() {
        let mut patch = Patch::default();
        let src = patch.add(ModuleKind::Mixer);
        let dst = patch.add(ModuleKind::Delay);
        patch.connect(PortRef { module: src, port: 0 }, PortRef { module: dst, port: 0 }).unwrap();
        let schedule = Schedule::compile(&patch).unwrap();
        let producer_out = schedule.steps.iter().find(|s| s.module == src).unwrap().outputs[0];
        let consumer_in = schedule.steps.iter().find(|s| s.module == dst).unwrap().inputs[0];
        assert_eq!(producer_out, consumer_in);
        assert!(producer_out.is_some());
    }

    #[test]
    fn running_a_schedule_twice_does_not_allocate_or_drift() {
        // The pool must survive repeated runs with buffers intact - the
        // take-and-return dance is easy to get wrong in a way that only shows
        // up on the second block.
        let mut patch = Patch::default();
        let src = patch.add(ModuleKind::Mixer);
        let add = patch.add(ModuleKind::Delay);
        patch.connect(PortRef { module: src, port: 0 }, PortRef { module: add, port: 0 }).unwrap();
        let schedule = Schedule::compile(&patch).unwrap();
        let mut pool = BufferPool::new(schedule.buffer_count, 64);
        let mut modules: Vec<Box<dyn Module>> = vec![Box::new(Const(2.0)), Box::new(AddOne)];

        for _ in 0..100 {
            run(&schedule, &mut pool, &mut modules, 64, 48_000.0, &[0.0; 32]);
        }

        let out = schedule.steps.iter().find(|s| s.module == add).unwrap().outputs[0].unwrap();
        assert_eq!(pool.buffer(out)[0], 3.0);
        assert_eq!(pool.buffer(out).len(), 64);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib schedule`
Expected: compile error — `Schedule`, `BufferPool`, `run` not found.

- [ ] **Step 3: Write the implementation**

```rust
// src/graph/schedule.rs
use crate::core::ids::ModuleId;
use crate::graph::module::{Module, ProcessCtx, MAX_INPUTS, MAX_OUTPUTS};
use crate::graph::patch::{spec_for, Patch, PatchError, PortRef};

#[derive(Clone, Copy, Debug)]
pub struct Step {
    pub module: ModuleId,
    /// Index into the module instance slice, so execution needs no lookup.
    pub module_index: usize,
    pub inputs: [Option<usize>; MAX_INPUTS],
    pub outputs: [Option<usize>; MAX_OUTPUTS],
}

/// A compiled, flat execution order with buffer indices already resolved.
///
/// Compilation happens off the audio thread. The finished schedule is handed
/// over by pointer swap and the outgoing one is dropped on the thread that
/// built it, so the audio thread never sorts, allocates or frees.
#[derive(Clone, Debug, Default)]
pub struct Schedule {
    pub steps: Vec<Step>,
    pub buffer_count: usize,
}

impl Schedule {
    pub fn compile(patch: &Patch) -> Result<Schedule, PatchError> {
        let order = patch.topological_order()?;

        // One buffer per output port. Reusing buffers would save memory at the
        // cost of a liveness analysis that is not worth its risk yet.
        let mut buffer_of: Vec<(PortRef, usize)> = Vec::new();
        let mut next_buffer = 0usize;
        for node in patch.nodes() {
            let spec = spec_for(node.kind);
            for port in 0..spec.outputs.len() {
                buffer_of.push((PortRef { module: node.id, port: port as u8 }, next_buffer));
                next_buffer += 1;
            }
        }
        let lookup = |p: PortRef| buffer_of.iter().find(|(r, _)| *r == p).map(|(_, i)| *i);

        let mut steps = Vec::with_capacity(order.len());
        for id in order {
            let kind = patch.kind(id).ok_or(PatchError::UnknownModule(id))?;
            let spec = spec_for(kind);

            let mut inputs = [None; MAX_INPUTS];
            for port in 0..spec.inputs.len().min(MAX_INPUTS) {
                let to = PortRef { module: id, port: port as u8 };
                if let Some(edge) = patch.edges().iter().find(|e| e.to == to) {
                    inputs[port] = lookup(edge.from);
                }
            }

            let mut outputs = [None; MAX_OUTPUTS];
            for port in 0..spec.outputs.len().min(MAX_OUTPUTS) {
                outputs[port] = lookup(PortRef { module: id, port: port as u8 });
            }

            let module_index = patch
                .nodes()
                .iter()
                .position(|n| n.id == id)
                .ok_or(PatchError::UnknownModule(id))?;

            steps.push(Step { module: id, module_index, inputs, outputs });
        }

        Ok(Schedule { steps, buffer_count: next_buffer })
    }
}

/// Pre-allocated audio buffers, one per output port.
pub struct BufferPool {
    bufs: Vec<Vec<f32>>,
}

impl BufferPool {
    pub fn new(buffer_count: usize, max_block: usize) -> Self {
        BufferPool { bufs: (0..buffer_count).map(|_| vec![0.0; max_block]).collect() }
    }

    pub fn buffer(&self, index: usize) -> &[f32] {
        &self.bufs[index]
    }

    pub fn clear(&mut self) {
        for b in &mut self.bufs {
            b.fill(0.0);
        }
    }
}

/// Execute one block. Allocation-free.
///
/// Output buffers are moved out of the pool before inputs are borrowed, then
/// moved back. Moving a `Vec` is a pointer copy, so this costs nothing and
/// avoids both `unsafe` and the aliasing problem of borrowing two elements of
/// one `Vec` at once.
pub fn run(
    schedule: &Schedule,
    pool: &mut BufferPool,
    modules: &mut [Box<dyn Module>],
    frames: usize,
    sample_rate: f32,
    params: &[f32],
) {
    for step in &schedule.steps {
        let mut owned: [Option<Vec<f32>>; MAX_OUTPUTS] = [const { None }; MAX_OUTPUTS];
        for (slot, index) in step.outputs.iter().enumerate() {
            if let Some(i) = index {
                owned[slot] = Some(std::mem::take(&mut pool.bufs[*i]));
            }
        }

        {
            let mut inputs: [Option<&[f32]>; MAX_INPUTS] = [None; MAX_INPUTS];
            for (slot, index) in step.inputs.iter().enumerate() {
                if let Some(i) = index {
                    inputs[slot] = Some(&pool.bufs[*i][..frames]);
                }
            }

            let mut outputs: [Option<&mut [f32]>; MAX_OUTPUTS] = [const { None }; MAX_OUTPUTS];
            for (slot, buf) in owned.iter_mut().enumerate() {
                if let Some(b) = buf {
                    outputs[slot] = Some(&mut b[..frames]);
                }
            }

            let mut ctx = ProcessCtx { frames, sample_rate, inputs, outputs, params };
            modules[step.module_index].process(&mut ctx);
        }

        for (slot, index) in step.outputs.iter().enumerate() {
            if let Some(i) = index {
                if let Some(b) = owned[slot].take() {
                    pool.bufs[*i] = b;
                }
            }
        }
    }
}
```

Add `pub mod schedule;` to `src/graph/mod.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib schedule`
Expected: 5 passing tests.

- [ ] **Step 5: Mutation-check the ordering and the buffer handover**

1. Reverse `schedule.steps` before running in `run` (iterate `.rev()`).
   Expected: `a_chain_executes_in_dependency_order` fails. Restore.
2. Skip the write-back loop at the end of `run`.
   Expected: `running_a_schedule_twice_does_not_allocate_or_drift` fails, because the second block finds an empty buffer. Restore.

- [ ] **Step 6: Commit**

```bash
git add src/graph
git commit -m "feat(graph): schedule compilation and buffer pool

Compilation happens off the audio thread; the finished schedule is handed
over and the outgoing one dropped on the thread that built it, so the audio
thread never sorts, allocates or frees.

Output buffers are moved out of the pool before inputs are borrowed and
moved back afterwards. Moving a Vec is a pointer copy, so this costs
nothing and avoids both unsafe and the aliasing problem of borrowing two
elements of one Vec at once. The write-back is the easy half to get wrong -
it only fails on the second block - so it is pinned and mutation-checked."
```

---

## Task 11: The global module set

**Files:**
- Create: `src/graph/modules/mod.rs`, `src/graph/modules/lfo.rs`, `src/graph/modules/mixer.rs`, `src/graph/modules/delay.rs`, `src/graph/modules/reverb.rs`, `src/graph/modules/output.rs`
- Modify: `src/graph/mod.rs`

**Interfaces:**
- Consumes: `Module`, `ProcessCtx`, `ModuleSpec` (Task 9); `spec_for`, `ModuleKind` (Task 9); `flush_denormal` (Task 8); `ParamRegistry` (Task 7)
- Produces:
  - `Lfo`, `Mixer`, `Delay`, `Reverb`, `Output` — each implementing `Module`
  - `make(kind: ModuleKind) -> Box<dyn Module>` — each module builds its own `ParamRegistry`, so no registry is threaded through
  - `Output::master_peak(&self) -> f32` — read after each block for the meter

The `params` slice a module receives is the **whole** normalised parameter array; a module reads it through its own spec's `params` list. Modules convert to real units with `ParamRegistry`, which each one holds a copy of the relevant descriptors from at `prepare` time. To keep this simple and allocation-free, each module stores its own `ParamRegistry` clone-free reference: `ParamRegistry` is cheap and `Copy`-like in practice, so each module owns one built in `make`.

- [ ] **Step 1: Write the failing tests**

```rust
// src/graph/modules/mod.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::module::{Module, ProcessCtx, MAX_INPUTS, MAX_OUTPUTS};
    use crate::params::registry::{ParamRegistry, PARAM_COUNT};

    const FRAMES: usize = 128;

    /// Run one module in isolation with a given input and normalised params.
    fn render(m: &mut dyn Module, input: Option<&[f32]>, params: &[f32]) -> Vec<f32> {
        let mut out_buf = vec![0.0f32; FRAMES];
        let mut inputs: [Option<&[f32]>; MAX_INPUTS] = [None; MAX_INPUTS];
        inputs[0] = input;
        let mut outputs: [Option<&mut [f32]>; MAX_OUTPUTS] = [const { None }; MAX_OUTPUTS];
        outputs[0] = Some(&mut out_buf[..]);
        let mut ctx = ProcessCtx { frames: FRAMES, sample_rate: 48_000.0, inputs, outputs, params };
        m.process(&mut ctx);
        out_buf
    }

    fn defaults() -> Vec<f32> {
        let reg = ParamRegistry::new();
        (0..PARAM_COUNT)
            .map(|i| {
                let id = crate::core::ids::ParamId(i as u16);
                reg.normalize(id, reg.desc(id).default)
            })
            .collect()
    }

    #[test]
    fn every_module_kind_can_be_built() {
        use crate::graph::patch::ModuleKind::*;
        for kind in [Lfo, Mixer, Delay, Reverb, Output] {
            let m = make(kind);
            assert_eq!(m.spec().name, crate::graph::patch::spec_for(kind).name);
        }
    }

    #[test]
    fn the_mixer_sums_its_inputs() {
        let mut m = make(crate::graph::patch::ModuleKind::Mixer);
        m.prepare(48_000.0, FRAMES);
        let a = vec![0.25f32; FRAMES];
        let out = render(m.as_mut(), Some(&a), &defaults());
        assert!((out[0] - 0.25).abs() < 1e-6);
    }

    #[test]
    fn a_module_overwrites_its_output_rather_than_accumulating() {
        // The contract every module obeys. If one accumulated, its output would
        // grow without bound across blocks and nothing would say why.
        let mut m = make(crate::graph::patch::ModuleKind::Mixer);
        m.prepare(48_000.0, FRAMES);
        let a = vec![0.25f32; FRAMES];
        let first = render(m.as_mut(), Some(&a), &defaults());
        let second = render(m.as_mut(), Some(&a), &defaults());
        assert!((first[0] - second[0]).abs() < 1e-6);
    }

    #[test]
    fn the_delay_repeats_an_impulse_later_not_immediately() {
        let mut m = make(crate::graph::patch::ModuleKind::Delay);
        m.prepare(48_000.0, FRAMES);
        let mut params = defaults();
        let reg = ParamRegistry::new();
        params[crate::params::registry::DELAY_TIME.0 as usize] =
            reg.normalize(crate::params::registry::DELAY_TIME, 0.001); // 48 samples
        params[crate::params::registry::DELAY_MIX.0 as usize] = 1.0;

        let mut input = vec![0.0f32; FRAMES];
        input[0] = 1.0;
        let out = render(m.as_mut(), Some(&input), &params);

        assert!(out[0].abs() < 0.9, "the echo arrived instantly: {}", out[0]);
        let echo = out[40..60].iter().fold(0.0f32, |a, b| a.max(b.abs()));
        assert!(echo > 0.1, "no echo appeared around the expected delay, peak was {echo}");
    }

    #[test]
    fn no_module_produces_a_nan_or_runs_away() {
        // A single NaN poisons everything downstream and the speakers go silent
        // with no error anywhere. Cheap to check, catastrophic to miss.
        use crate::graph::patch::ModuleKind::*;
        for kind in [Lfo, Mixer, Delay, Reverb, Output] {
            let mut m = make(kind);
            m.prepare(48_000.0, FRAMES);
            let mut noisy = vec![0.0f32; FRAMES];
            for (i, v) in noisy.iter_mut().enumerate() {
                *v = if i % 2 == 0 { 0.9 } else { -0.9 };
            }
            for _ in 0..200 {
                let out = render(m.as_mut(), Some(&noisy), &defaults());
                for v in &out {
                    assert!(v.is_finite(), "{:?} produced {v}", kind);
                    assert!(v.abs() < 20.0, "{:?} ran away to {v}", kind);
                }
            }
        }
    }

    #[test]
    fn the_output_module_reports_the_peak_it_saw() {
        let mut m = Output::default();
        m.prepare(48_000.0, FRAMES);
        let mut input = vec![0.0f32; FRAMES];
        input[10] = 0.5;
        let mut params = defaults();
        params[crate::params::registry::MASTER_GAIN.0 as usize] = 1.0;
        render(&mut m, Some(&input), &params);
        assert!((m.master_peak() - 0.5).abs() < 1e-3);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib modules`
Expected: compile error — `make`, `Output` not found.

- [ ] **Step 3: Write the implementation**

```rust
// src/graph/modules/mixer.rs
use crate::graph::module::{Module, ModuleSpec, ProcessCtx};
use crate::graph::patch::{spec_for, ModuleKind};

/// Sums its inputs. Summing is explicit and visible in the patch, which is why
/// a plain input port refuses a second cable.
#[derive(Default)]
pub struct Mixer;

impl Module for Mixer {
    fn spec(&self) -> &'static ModuleSpec { spec_for(ModuleKind::Mixer) }
    fn prepare(&mut self, _sample_rate: f32, _max_block: usize) {}
    fn reset(&mut self) {}

    fn process(&mut self, ctx: &mut ProcessCtx) {
        let frames = ctx.frames;
        let a: Option<Vec<f32>> = ctx.input(0).map(|s| s[..frames].to_vec());
        let b: Option<Vec<f32>> = ctx.input(1).map(|s| s[..frames].to_vec());
        if let Some(out) = ctx.output(0) {
            for i in 0..frames {
                out[i] = a.as_ref().map_or(0.0, |s| s[i]) + b.as_ref().map_or(0.0, |s| s[i]);
            }
        }
    }
}
```

**Note for the implementer:** the `to_vec()` calls above allocate and are therefore **not acceptable in the final code**. They are shown to make the intent obvious. Replace them with a fixed scratch buffer allocated in `prepare`:

```rust
pub struct Mixer { scratch_a: Vec<f32>, scratch_b: Vec<f32> }

impl Module for Mixer {
    fn prepare(&mut self, _sr: f32, max_block: usize) {
        self.scratch_a = vec![0.0; max_block];
        self.scratch_b = vec![0.0; max_block];
    }
    fn process(&mut self, ctx: &mut ProcessCtx) {
        let frames = ctx.frames;
        match ctx.input(0) { Some(s) => self.scratch_a[..frames].copy_from_slice(&s[..frames]),
                             None => self.scratch_a[..frames].fill(0.0) }
        match ctx.input(1) { Some(s) => self.scratch_b[..frames].copy_from_slice(&s[..frames]),
                             None => self.scratch_b[..frames].fill(0.0) }
        let (a, b) = (&self.scratch_a, &self.scratch_b);
        if let Some(out) = ctx.output(0) {
            for i in 0..frames { out[i] = a[i] + b[i]; }
        }
    }
}
```

Apply the same pattern in every module that needs to read an input while writing an output. `cargo clippy` will not catch this — **grep the finished `src/graph/modules/` for `to_vec`, `collect`, `vec!` and `String` before committing**, and confirm the only hits are inside `prepare` or `#[cfg(test)]`.

```rust
// src/graph/modules/lfo.rs
use crate::core::ids::ParamId;
use crate::graph::module::{Module, ModuleSpec, ProcessCtx};
use crate::graph::patch::{spec_for, ModuleKind};
use crate::params::registry::{ParamRegistry, LFO_AMOUNT, LFO_RATE};
use std::f32::consts::TAU;

pub struct Lfo { phase: f32, registry: ParamRegistry }

impl Default for Lfo {
    fn default() -> Self { Lfo { phase: 0.0, registry: ParamRegistry::new() } }
}

impl Module for Lfo {
    fn spec(&self) -> &'static ModuleSpec { spec_for(ModuleKind::Lfo) }
    fn prepare(&mut self, _sr: f32, _max_block: usize) {}
    fn reset(&mut self) { self.phase = 0.0; }

    fn process(&mut self, ctx: &mut ProcessCtx) {
        let read = |id: ParamId| ctx.params.get(id.0 as usize).copied().unwrap_or(0.0);
        let rate = self.registry.denormalize(LFO_RATE, read(LFO_RATE));
        let amount = self.registry.denormalize(LFO_AMOUNT, read(LFO_AMOUNT));
        let inc = rate / ctx.sample_rate;
        let frames = ctx.frames;
        let mut phase = self.phase;
        if let Some(out) = ctx.output(0) {
            for i in 0..frames {
                out[i] = (phase * TAU).sin() * amount;
                phase += inc;
                if phase >= 1.0 { phase -= 1.0; }
            }
        } else {
            phase = (phase + inc * frames as f32).fract();
        }
        self.phase = phase;
    }
}
```

```rust
// src/graph/modules/delay.rs
use crate::audio::dsp::flush_denormal;
use crate::core::ids::ParamId;
use crate::graph::module::{Module, ModuleSpec, ProcessCtx};
use crate::graph::patch::{spec_for, ModuleKind};
use crate::params::registry::{ParamRegistry, DELAY_FEEDBACK, DELAY_MIX, DELAY_TIME};

/// The longest delay the buffer can hold, in seconds. Allocated once in
/// `prepare`; changing the time only moves a read pointer.
const MAX_DELAY_S: f32 = 2.0;

pub struct Delay {
    buffer: Vec<f32>,
    write: usize,
    scratch: Vec<f32>,
    registry: ParamRegistry,
}

impl Default for Delay {
    fn default() -> Self {
        Delay { buffer: Vec::new(), write: 0, scratch: Vec::new(), registry: ParamRegistry::new() }
    }
}

impl Module for Delay {
    fn spec(&self) -> &'static ModuleSpec { spec_for(ModuleKind::Delay) }

    fn prepare(&mut self, sample_rate: f32, max_block: usize) {
        self.buffer = vec![0.0; (sample_rate * MAX_DELAY_S) as usize + 1];
        self.scratch = vec![0.0; max_block];
        self.write = 0;
    }

    fn reset(&mut self) { self.buffer.fill(0.0); self.write = 0; }

    fn process(&mut self, ctx: &mut ProcessCtx) {
        let frames = ctx.frames;
        match ctx.input(0) {
            Some(s) => self.scratch[..frames].copy_from_slice(&s[..frames]),
            None => self.scratch[..frames].fill(0.0),
        }
        let read = |id: ParamId| ctx.params.get(id.0 as usize).copied().unwrap_or(0.0);
        let time = self.registry.denormalize(DELAY_TIME, read(DELAY_TIME));
        let feedback = self.registry.denormalize(DELAY_FEEDBACK, read(DELAY_FEEDBACK));
        let mix = self.registry.denormalize(DELAY_MIX, read(DELAY_MIX));

        let len = self.buffer.len();
        let offset = ((time * ctx.sample_rate) as usize).clamp(1, len - 1);

        if let Some(out) = ctx.output(0) {
            for i in 0..frames {
                let dry = self.scratch[i];
                let read_at = (self.write + len - offset) % len;
                let wet = self.buffer[read_at];
                self.buffer[self.write] = flush_denormal(dry + wet * feedback);
                self.write = (self.write + 1) % len;
                out[i] = dry * (1.0 - mix) + wet * mix;
            }
        }
    }
}
```

```rust
// src/graph/modules/reverb.rs
use crate::audio::dsp::flush_denormal;
use crate::core::ids::ParamId;
use crate::graph::module::{Module, ModuleSpec, ProcessCtx};
use crate::graph::patch::{spec_for, ModuleKind};
use crate::params::registry::{ParamRegistry, REVERB_MIX, REVERB_SIZE};

/// Four comb filters into two all-passes: a Schroeder reverb.
///
/// Chosen because it is small, well understood, and sounds like a room. Delay
/// lengths are mutually prime so the combs do not reinforce each other into a
/// ringing pitch.
const COMB_LENS: [usize; 4] = [1_557, 1_617, 1_491, 1_422];
const ALLPASS_LENS: [usize; 2] = [225, 556];

pub struct Reverb {
    combs: [Vec<f32>; 4],
    comb_pos: [usize; 4],
    allpasses: [Vec<f32>; 2],
    allpass_pos: [usize; 2],
    scratch: Vec<f32>,
    registry: ParamRegistry,
}

impl Default for Reverb {
    fn default() -> Self {
        Reverb {
            combs: Default::default(),
            comb_pos: [0; 4],
            allpasses: Default::default(),
            allpass_pos: [0; 2],
            scratch: Vec::new(),
            registry: ParamRegistry::new(),
        }
    }
}

impl Module for Reverb {
    fn spec(&self) -> &'static ModuleSpec { spec_for(ModuleKind::Reverb) }

    fn prepare(&mut self, sample_rate: f32, max_block: usize) {
        // Delay lengths are quoted for 44.1 kHz; scale so the room does not
        // change size with the audio device.
        let scale = sample_rate / 44_100.0;
        for (i, len) in COMB_LENS.iter().enumerate() {
            self.combs[i] = vec![0.0; (*len as f32 * scale) as usize + 1];
        }
        for (i, len) in ALLPASS_LENS.iter().enumerate() {
            self.allpasses[i] = vec![0.0; (*len as f32 * scale) as usize + 1];
        }
        self.scratch = vec![0.0; max_block];
        self.comb_pos = [0; 4];
        self.allpass_pos = [0; 2];
    }

    fn reset(&mut self) {
        for c in &mut self.combs { c.fill(0.0); }
        for a in &mut self.allpasses { a.fill(0.0); }
    }

    fn process(&mut self, ctx: &mut ProcessCtx) {
        let frames = ctx.frames;
        match ctx.input(0) {
            Some(s) => self.scratch[..frames].copy_from_slice(&s[..frames]),
            None => self.scratch[..frames].fill(0.0),
        }
        let read = |id: ParamId| ctx.params.get(id.0 as usize).copied().unwrap_or(0.0);
        let size = self.registry.denormalize(REVERB_SIZE, read(REVERB_SIZE));
        let mix = self.registry.denormalize(REVERB_MIX, read(REVERB_MIX));
        // Kept below 1.0 with headroom: at 1.0 the combs never decay.
        let feedback = 0.7 + size * 0.28;

        if let Some(out) = ctx.output(0) {
            for i in 0..frames {
                let dry = self.scratch[i];
                let mut wet = 0.0;
                for c in 0..4 {
                    let pos = self.comb_pos[c];
                    let delayed = self.combs[c][pos];
                    wet += delayed;
                    self.combs[c][pos] = flush_denormal(dry + delayed * feedback);
                    self.comb_pos[c] = (pos + 1) % self.combs[c].len();
                }
                wet *= 0.25;
                for a in 0..2 {
                    let pos = self.allpass_pos[a];
                    let delayed = self.allpasses[a][pos];
                    let output = delayed - wet;
                    self.allpasses[a][pos] = flush_denormal(wet + delayed * 0.5);
                    self.allpass_pos[a] = (pos + 1) % self.allpasses[a].len();
                    wet = output;
                }
                out[i] = dry * (1.0 - mix) + wet * mix;
            }
        }
    }
}
```

```rust
// src/graph/modules/output.rs
use crate::core::ids::ParamId;
use crate::graph::module::{Module, ModuleSpec, ProcessCtx};
use crate::graph::patch::{spec_for, ModuleKind};
use crate::params::registry::{ParamRegistry, MASTER_GAIN};

/// The terminal node. It has no output port; the engine reads its scratch
/// buffer to fill the device.
pub struct Output {
    pub buffer: Vec<f32>,
    peak: f32,
    registry: ParamRegistry,
}

impl Default for Output {
    fn default() -> Self {
        Output { buffer: Vec::new(), peak: 0.0, registry: ParamRegistry::new() }
    }
}

impl Output {
    /// The loudest sample of the most recent block, for the meter.
    pub fn master_peak(&self) -> f32 { self.peak }
}

impl Module for Output {
    fn spec(&self) -> &'static ModuleSpec { spec_for(ModuleKind::Output) }

    fn prepare(&mut self, _sample_rate: f32, max_block: usize) {
        self.buffer = vec![0.0; max_block];
    }

    fn reset(&mut self) { self.buffer.fill(0.0); self.peak = 0.0; }

    fn process(&mut self, ctx: &mut ProcessCtx) {
        let frames = ctx.frames;
        let read = |id: ParamId| ctx.params.get(id.0 as usize).copied().unwrap_or(0.0);
        let gain = self.registry.denormalize(MASTER_GAIN, read(MASTER_GAIN));
        let mut peak = 0.0f32;
        match ctx.input(0) {
            Some(s) => {
                for i in 0..frames {
                    // A soft clip rather than a hard one: a runaway patch should
                    // sound wrong, not damage anything or produce a digital
                    // spike that lands like a click.
                    let v = (s[i] * gain).clamp(-1.5, 1.5).tanh();
                    self.buffer[i] = v;
                    peak = peak.max(v.abs());
                }
            }
            None => self.buffer[..frames].fill(0.0),
        }
        self.peak = peak;
    }
}
```

```rust
// src/graph/modules/mod.rs
pub mod delay;
pub mod lfo;
pub mod mixer;
pub mod output;
pub mod reverb;

pub use delay::Delay;
pub use lfo::Lfo;
pub use mixer::Mixer;
pub use output::Output;
pub use reverb::Reverb;

use crate::graph::module::Module;
use crate::graph::patch::ModuleKind;

/// Build a module instance. Voice-zone kinds are not executed through the graph
/// in M1a - `audio::voice` runs that chain directly - so asking for one here is
/// a programming error rather than a runtime condition.
pub fn make(kind: ModuleKind) -> Box<dyn Module> {
    match kind {
        ModuleKind::Lfo => Box::new(Lfo::default()),
        ModuleKind::Mixer => Box::new(Mixer::default()),
        ModuleKind::Delay => Box::new(Delay::default()),
        ModuleKind::Reverb => Box::new(Reverb::default()),
        ModuleKind::Output => Box::new(Output::default()),
        ModuleKind::Oscillator | ModuleKind::Filter | ModuleKind::Envelope | ModuleKind::Vca => {
            panic!("{:?} is a voice-zone module and is not executed through the graph in M1a", kind)
        }
    }
}
```

Add `pub mod modules;` to `src/graph/mod.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib modules`
Expected: 6 passing tests.

- [ ] **Step 5: Verify there is no allocation left in any `process`**

Run: `grep -nE "to_vec|\.collect\(|vec!\[|String::" src/graph/modules/*.rs`
Expected: hits only inside `prepare` bodies or `#[cfg(test)]` blocks. Any hit inside a `process` is a violation of the plan's global constraints and must be replaced with a scratch buffer allocated in `prepare`.

Then mutation-check the reverb's stability guard: change `let feedback = 0.7 + size * 0.28;` to `1.0 + size`.
Expected: `no_module_produces_a_nan_or_runs_away` fails on the reverb. Restore.

- [ ] **Step 6: Commit**

```bash
git add src/graph
git commit -m "feat(graph): global module set - LFO, mixer, delay, reverb, output

Every module overwrites its outputs rather than accumulating; that contract
is pinned, because a module that accumulated would grow without bound across
blocks and nothing would say why.

Reverb comb lengths are mutually prime so the combs cannot reinforce into a
ringing pitch, and are scaled by sample rate so the room does not change
size with the audio device. Feedback stays below 1.0 with headroom - at 1.0
the combs never decay, which the runaway test catches.

Output soft-clips through tanh rather than hard-clipping: a runaway patch
should sound wrong, not produce a digital spike that lands like a click.

All allocation lives in prepare. Verified by grep, since clippy does not
catch this and the audio thread cannot afford it."
```

---

## Task 12: Audio host, command queue, telemetry — first sound

**Files:**
- Create: `src/engine/mod.rs`, `src/engine/command.rs`, `src/engine/telemetry.rs`, `src/engine/host.rs`
- Modify: `src/main.rs` (add `mod engine;`), `src/app.rs`, `src/ui/debug.rs`

**Interfaces:**
- Consumes: `Action` (Task 5), `MacroId`/`ParamId` (Task 3)
- Produces:
  - `AudioCommand` — `Copy`, wrapping `Action` plus `SetTestTone(bool)`
  - `EngineEvent { NoteStarted { note: u8 }, NoteEnded { note: u8 }, VoiceStolen }`
  - `Telemetry` with `Arc`-shared atomics: `peak`, `active_voices`, `dropped_commands`, `dropped_events`, `underruns`, `dsp_load_permille`, plus `push_command(&self, cmd) -> bool`
  - `AudioHost::start(device: Option<&str>) -> Result<AudioHost, HostError>`, fields `sample_rate`, `buffer_frames`, `latency_ms`, `telemetry`, `commands`, `events`, and `AudioHost::devices() -> Vec<String>`

- [ ] **Step 1: Write the failing tests**

```rust
// src/engine/telemetry.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::event::{Action, TransportCmd};

    #[test]
    fn a_full_queue_drops_and_counts_rather_than_blocking() {
        // Blocking here would stall whichever input thread pushed, and a
        // silently dropped command is a note that never sounds with nothing to
        // show for it. The counter is the only way to learn it happened.
        let t = Telemetry::new(4);
        for _ in 0..4 {
            assert!(t.push_command(AudioCommand::Act(Action::Transport(TransportCmd::Play))));
        }
        assert!(!t.push_command(AudioCommand::Act(Action::Transport(TransportCmd::Play))));
        assert_eq!(t.dropped_commands(), 1);
    }

    #[test]
    fn draining_makes_room_again() {
        let t = Telemetry::new(2);
        t.push_command(AudioCommand::SetTestTone(true));
        assert!(t.commands.pop().is_some());
        assert!(t.push_command(AudioCommand::SetTestTone(false)));
    }

    #[test]
    fn peak_survives_the_trip_through_an_atomic() {
        let t = Telemetry::new(4);
        t.set_peak(0.625);
        assert!((t.peak() - 0.625).abs() < 1e-3);
    }

    #[test]
    fn every_command_is_copy() {
        fn assert_copy<T: Copy>() {}
        assert_copy::<AudioCommand>();
        assert_copy::<EngineEvent>();
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib telemetry`
Expected: compile error — `Telemetry`, `AudioCommand` not found.

- [ ] **Step 3: Write the implementation**

```rust
// src/engine/command.rs
use crate::core::event::Action;

/// What the audio thread is asked to do.
///
/// Every variant is `Copy`. If one ever needs owned data, that is a design
/// question to raise rather than a constraint to work around: the queue is
/// allocation-free and freeing on the audio thread is forbidden.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum AudioCommand {
    Act(Action),
    /// A continuous 440 Hz tone. Kept permanently as a diagnostic: it proves
    /// the whole path from device selection to speaker independently of whether
    /// any input is working.
    SetTestTone(bool),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EngineEvent {
    NoteStarted { note: u8 },
    NoteEnded { note: u8 },
    VoiceStolen { note: u8 },
}
```

```rust
// src/engine/telemetry.rs
use crossbeam_queue::ArrayQueue;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

pub use crate::engine::command::{AudioCommand, EngineEvent};

/// Shared between every thread. Continuous values live in atomics rather than
/// in the event queue: pushing a level meter at two hundred hertz would flood
/// the queue and starve real events.
pub struct Telemetry {
    pub commands: ArrayQueue<AudioCommand>,
    pub events: ArrayQueue<EngineEvent>,
    peak_milli: AtomicU32,
    active_voices: AtomicU32,
    dsp_load_permille: AtomicU32,
    dropped_commands: AtomicU64,
    dropped_events: AtomicU64,
    underruns: AtomicU64,
}

impl Telemetry {
    pub fn new(capacity: usize) -> Arc<Telemetry> {
        Arc::new(Telemetry {
            commands: ArrayQueue::new(capacity),
            events: ArrayQueue::new(capacity),
            peak_milli: AtomicU32::new(0),
            active_voices: AtomicU32::new(0),
            dsp_load_permille: AtomicU32::new(0),
            dropped_commands: AtomicU64::new(0),
            dropped_events: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
        })
    }

    /// Returns false when the queue was full. Never blocks: blocking here would
    /// stall the input thread that pushed.
    pub fn push_command(&self, cmd: AudioCommand) -> bool {
        match self.commands.push(cmd) {
            Ok(()) => true,
            Err(_) => {
                self.dropped_commands.fetch_add(1, Ordering::Relaxed);
                false
            }
        }
    }

    pub fn push_event(&self, ev: EngineEvent) {
        if self.events.push(ev).is_err() {
            self.dropped_events.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn set_peak(&self, v: f32) {
        self.peak_milli.store((v.clamp(0.0, 4.0) * 1000.0) as u32, Ordering::Relaxed);
    }

    pub fn peak(&self) -> f32 {
        self.peak_milli.load(Ordering::Relaxed) as f32 / 1000.0
    }

    pub fn set_active_voices(&self, n: u32) { self.active_voices.store(n, Ordering::Relaxed); }
    pub fn active_voices(&self) -> u32 { self.active_voices.load(Ordering::Relaxed) }

    pub fn set_dsp_load(&self, permille: u32) { self.dsp_load_permille.store(permille, Ordering::Relaxed); }
    pub fn dsp_load_percent(&self) -> f32 { self.dsp_load_permille.load(Ordering::Relaxed) as f32 / 10.0 }

    pub fn note_underrun(&self) { self.underruns.fetch_add(1, Ordering::Relaxed); }
    pub fn underruns(&self) -> u64 { self.underruns.load(Ordering::Relaxed) }
    pub fn dropped_commands(&self) -> u64 { self.dropped_commands.load(Ordering::Relaxed) }
    pub fn dropped_events(&self) -> u64 { self.dropped_events.load(Ordering::Relaxed) }
}
```

```rust
// src/engine/host.rs
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
        stream_config.buffer_size = cpal::BufferSize::Fixed(256);

        let telemetry = Telemetry::new(1024);
        let mut engine = AudioEngine::new(sample_rate, 2048, Arc::clone(&telemetry));

        let cb_telemetry = Arc::clone(&telemetry);
        let build = device.build_output_stream(
            &stream_config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let started = std::time::Instant::now();
                let frames = data.len() / channels;
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
                let telemetry2 = Arc::clone(&telemetry);
                let mut engine2 = AudioEngine::new(sample_rate, 4096, Arc::clone(&telemetry));
                device
                    .build_output_stream(
                        &fallback,
                        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                            let frames = data.len() / channels;
                            engine2.render(frames);
                            let out = engine2.output();
                            for (i, frame) in data.chunks_mut(channels).enumerate() {
                                let v = out.get(i).copied().unwrap_or(0.0);
                                for sample in frame.iter_mut() { *sample = v; }
                            }
                            telemetry2.set_peak(0.0);
                        },
                        move |err| log::error!("audio stream error: {err}"),
                        None,
                    )
                    .map_err(|e| HostError::Stream(e.to_string()))?
            }
        };

        stream.play().map_err(|e| HostError::Stream(e.to_string()))?;

        Ok(AudioHost {
            _stream: stream,
            device_name,
            sample_rate,
            buffer_frames: 256,
            telemetry,
        })
    }
}
```

```rust
// src/engine/mod.rs
pub mod command;
pub mod host;
pub mod telemetry;
```

Add `mod engine;` to `src/main.rs`.

**`host.rs` refers to `AudioEngine`, which Task 13 builds properly. Task 12 must still compile and make sound**, so create the minimal version now and grow it in Task 13:

```rust
// src/audio/engine.rs  — minimal; Task 13 replaces the body, not the signature
use std::sync::Arc;
use crate::engine::telemetry::{AudioCommand, Telemetry};

pub struct AudioEngine {
    sample_rate: f32,
    pub telemetry: Arc<Telemetry>,
    out: Vec<f32>,
    test_tone: bool,
    test_phase: f32,
}

impl AudioEngine {
    pub fn new(sample_rate: f32, max_block: usize, telemetry: Arc<Telemetry>) -> AudioEngine {
        AudioEngine {
            sample_rate,
            telemetry,
            out: vec![0.0; max_block],
            test_tone: false,
            test_phase: 0.0,
        }
    }

    pub fn render(&mut self, frames: usize) {
        let frames = frames.min(self.out.len());
        while let Some(cmd) = self.telemetry.commands.pop() {
            if let AudioCommand::SetTestTone(on) = cmd {
                self.test_tone = on;
            }
        }
        self.out[..frames].fill(0.0);
        if self.test_tone {
            let inc = 440.0 / self.sample_rate;
            let mut peak = 0.0f32;
            for s in self.out[..frames].iter_mut() {
                *s = (self.test_phase * std::f32::consts::TAU).sin() * 0.2;
                peak = peak.max(s.abs());
                self.test_phase += inc;
                if self.test_phase >= 1.0 {
                    self.test_phase -= 1.0;
                }
            }
            self.telemetry.set_peak(peak);
        } else {
            self.telemetry.set_peak(0.0);
        }
    }

    pub fn output(&self) -> &[f32] { &self.out }
}
```

Add `pub mod engine;` to `src/audio/mod.rs`.

Wire the host into `FluxApp`: hold `Option<AudioHost>` and an error string, start it in `FluxApp::new`, and show the failure in the interface rather than panicking. Add a test-tone toggle to `ui::debug::show`:

```rust
// src/ui/debug.rs
use crate::engine::telemetry::{AudioCommand, Telemetry};

pub fn show(ui: &mut egui::Ui, host: Option<&crate::engine::host::AudioHost>,
            error: Option<&str>, test_tone: &mut bool) {
    ui.heading("DIAGNOSTICS");
    if let Some(err) = error {
        ui.colored_label(crate::ui::theme::DANGER, err);
        ui.label("FLUX is running without audio. Choose another device in Settings.");
        return;
    }
    let Some(host) = host else { return };
    let t: &Telemetry = &host.telemetry;

    egui::Grid::new("diag").num_columns(2).spacing([24.0, 6.0]).show(ui, |ui| {
        ui.label("Device");           ui.label(&host.device_name);              ui.end_row();
        ui.label("Sample rate");      ui.label(format!("{:.0} Hz", host.sample_rate)); ui.end_row();
        ui.label("Buffer");           ui.label(format!("{} frames", host.buffer_frames)); ui.end_row();
        ui.label("Latency");          ui.label(format!("{:.1} ms", host.latency_ms())); ui.end_row();
        ui.label("DSP load");         ui.label(format!("{:.1} %", t.dsp_load_percent())); ui.end_row();
        ui.label("Active voices");    ui.label(t.active_voices().to_string());  ui.end_row();
        ui.label("Peak");             ui.label(format!("{:.3}", t.peak()));     ui.end_row();
        ui.label("Dropped commands"); ui.label(t.dropped_commands().to_string()); ui.end_row();
        ui.label("Dropped events");   ui.label(t.dropped_events().to_string());  ui.end_row();
        ui.label("Underruns");        ui.label(t.underruns().to_string());       ui.end_row();
    });

    ui.separator();
    if ui.checkbox(test_tone, "Test tone (440 Hz)").changed() {
        t.push_command(AudioCommand::SetTestTone(*test_tone));
    }
    ui.small("Proves the path from device to speaker without needing any input to work.");
}
```

- [ ] **Step 4: Run the tests and hear the tone**

Run: `cargo test --lib telemetry` — expected: 4 passing tests.
Run: `cargo run`, open DIAGNOSTICS, tick **Test tone**.
Expected: a steady 440 Hz tone, and the latency figure showing a real number. Untick it; silence returns.

If the device list is empty or the stream fails, the diagnostics view must show the error text and the window must still work. Verify that by starting with `start(Some("no such device"))` temporarily — it should fall back to the default device, not crash.

- [ ] **Step 5: Mutation-check the drop counter**

Change `push_command` to loop until the queue accepts (`while self.commands.push(cmd).is_err() {}`).
Expected: `a_full_queue_drops_and_counts_rather_than_blocking` hangs — kill it after a few seconds. That hang **is** the finding: it demonstrates exactly the stall the non-blocking design avoids. Restore, and note it in the commit message.

- [ ] **Step 6: Commit**

```bash
git add src/engine src/ui src/app.rs src/main.rs
git commit -m "feat(engine): CPAL host, lock-free command queue, telemetry

First sound. A test tone toggle in the diagnostics view proves the path
from device selection to speaker independently of any input working, so it
stays permanently rather than being scaffolding.

Continuous values live in atomics, not in the event queue: pushing a level
meter at 200 Hz would flood the queue and starve real events. Drop counters
increment on every failed push, because a counter cannot be retrofitted
into a path that already discarded silently.

Mutation-checking the queue was instructive: making push_command block
until accepted turns the test into a hang, which is precisely the input
thread stall the non-blocking design exists to avoid.

A device error logs and continues rather than taking the process down, and
a refused buffer size falls back to the device's preference instead of
refusing to make sound."
```

---

## Task 13: Voices and the two-zone engine

Replaces the minimal engine from Task 12 with the real one: sixteen voices feeding the scheduled global graph.

**Files:**
- Create: `src/audio/voice.rs`
- Modify: `src/audio/engine.rs` (grow it from the Task 12 stub), `src/audio/mod.rs`

**Interfaces:**
- Consumes: `Osc`, `Svf`, `Adsr`, `AdsrParams`, `Waveform` (Task 8); `ModMatrix`, `ParamRegistry`, param constants, `MACROS`, `macro_source`, `MOD_SOURCE_COUNT` (Task 7); `Schedule`, `BufferPool`, `run` (Task 10); `Patch`, `ModuleKind`, `PortRef` (Task 9); `make` (Task 11); `AudioCommand`, `EngineEvent`, `Telemetry` (Task 12); `note_to_freq` (Task 3)
- Produces:
  - `VOICE_COUNT: usize = 16`
  - `Voice` with `note: Option<u8>`, `start(note, velocity, sample_rate)`, `release()`, `is_idle()`, `render(&mut self, out: &mut [f32], params: &ParamValues, sample_rate: f32)`
  - `VoicePool::note_on`, `note_off`, `render`, `active_count`
  - `AudioEngine::new(sample_rate, max_block, Arc<Telemetry>)`, `render(&mut self, frames: usize)`, `output(&self) -> &[f32]`, `active_notes(&self) -> u16` (bitmask over the twelve pitch classes)

- [ ] **Step 1: Write the failing tests**

```rust
// src/audio/voice.rs
#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn params() -> ParamValues {
        let reg = crate::params::registry::ParamRegistry::new();
        let mut v = ParamValues::default();
        for i in 0..crate::params::registry::PARAM_COUNT {
            let id = crate::core::ids::ParamId(i as u16);
            v.0[i] = reg.normalize(id, reg.desc(id).default);
        }
        v
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |a, b| a.max(b.abs()))
    }

    #[test]
    fn a_started_voice_makes_sound_and_an_idle_one_does_not() {
        let mut v = Voice::default();
        let mut buf = vec![0.0f32; 512];
        v.render(&mut buf, &params(), SR);
        assert_eq!(peak(&buf), 0.0, "an idle voice produced sound");

        v.start(60, 1.0, SR);
        buf.fill(0.0);
        v.render(&mut buf, &params(), SR);
        assert!(peak(&buf) > 0.01, "a started voice was silent");
    }

    #[test]
    fn a_released_voice_eventually_becomes_idle_and_free() {
        let mut v = Voice::default();
        v.start(60, 1.0, SR);
        let mut buf = vec![0.0f32; 512];
        v.render(&mut buf, &params(), SR);
        v.release();
        for _ in 0..200 {
            v.render(&mut buf, &params(), SR);
        }
        assert!(v.is_idle(), "the voice never freed itself");
        assert_eq!(v.note, None);
    }

    #[test]
    fn the_pool_gives_each_note_its_own_voice() {
        let mut pool = VoicePool::default();
        pool.note_on(60, 1.0, SR);
        pool.note_on(64, 1.0, SR);
        pool.note_on(67, 1.0, SR);
        assert_eq!(pool.active_count(), 3);
    }

    #[test]
    fn releasing_a_note_frees_only_that_voice() {
        let mut pool = VoicePool::default();
        pool.note_on(60, 1.0, SR);
        pool.note_on(64, 1.0, SR);
        pool.note_off(60);
        let mut buf = vec![0.0f32; 512];
        for _ in 0..200 {
            pool.render(&mut buf, &params(), SR);
        }
        assert_eq!(pool.active_count(), 1);
    }

    #[test]
    fn retriggering_a_sounding_note_reuses_its_voice() {
        // Otherwise holding a key that repeats would consume the whole pool.
        let mut pool = VoicePool::default();
        pool.note_on(60, 1.0, SR);
        pool.note_on(60, 1.0, SR);
        assert_eq!(pool.active_count(), 1);
    }

    #[test]
    fn the_pool_steals_the_oldest_voice_when_full() {
        let mut pool = VoicePool::default();
        for i in 0..VOICE_COUNT {
            pool.note_on(40 + i as u8, 1.0, SR);
        }
        assert_eq!(pool.active_count(), VOICE_COUNT);
        pool.note_on(100, 1.0, SR);
        assert_eq!(pool.active_count(), VOICE_COUNT, "the pool grew past its limit");
        // The first note played is the one that gave way.
        assert!(!pool.voices.iter().any(|v| v.note == Some(40)));
        assert!(pool.voices.iter().any(|v| v.note == Some(100)));
    }

    #[test]
    fn note_off_for_a_note_that_is_not_sounding_is_harmless() {
        let mut pool = VoicePool::default();
        pool.note_off(60);
        assert_eq!(pool.active_count(), 0);
    }

    #[test]
    fn sixteen_simultaneous_voices_stay_in_range() {
        // Sixteen voices at full level would sum to sixteen. The pool must scale
        // so a chord does not clip before it reaches the mixer.
        let mut pool = VoicePool::default();
        for i in 0..VOICE_COUNT {
            pool.note_on(48 + i as u8, 1.0, SR);
        }
        let mut buf = vec![0.0f32; 512];
        for _ in 0..20 {
            buf.fill(0.0);
            pool.render(&mut buf, &params(), SR);
            for v in &buf {
                assert!(v.is_finite());
                assert!(v.abs() < 4.0, "sixteen voices summed to {v}");
            }
        }
    }
}
```

```rust
// src/audio/engine.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::event::Action;
    use crate::engine::telemetry::{AudioCommand, Telemetry};

    fn engine() -> AudioEngine {
        AudioEngine::new(48_000.0, 512, Telemetry::new(64))
    }

    #[test]
    fn a_fresh_engine_renders_silence() {
        let mut e = engine();
        e.render(256);
        assert!(e.output()[..256].iter().all(|v| *v == 0.0));
    }

    #[test]
    fn a_note_on_command_produces_sound() {
        let mut e = engine();
        e.telemetry.push_command(AudioCommand::Act(Action::NoteOn { note: 60, velocity: 1.0 }));
        e.render(256);
        let peak = e.output()[..256].iter().fold(0.0f32, |a, b| a.max(b.abs()));
        assert!(peak > 0.001, "the engine stayed silent, peak {peak}");
    }

    #[test]
    fn commands_are_drained_every_block_not_one_per_block() {
        // Sixteen notes pushed together must all sound in the same block. A
        // one-per-block drain would spread a chord over a quarter of a second.
        let mut e = engine();
        for note in 48..64u8 {
            e.telemetry.push_command(AudioCommand::Act(Action::NoteOn { note, velocity: 0.8 }));
        }
        e.render(64);
        assert_eq!(e.telemetry.active_voices(), 16);
    }

    #[test]
    fn a_macro_reaches_the_parameter_it_is_routed_to() {
        let mut e = engine();
        let before = e.params.value(crate::params::registry::FILTER_CUTOFF);
        e.telemetry.push_command(AudioCommand::Act(Action::SetMacro {
            macro_id: crate::core::ids::MacroId(0),
            value: 1.0,
        }));
        e.render(64);
        let after = e.params.value(crate::params::registry::FILTER_CUTOFF);
        assert!(after > before, "BRIGHT did not open the filter: {before} then {after}");
    }

    #[test]
    fn a_negative_bipolar_macro_moves_the_other_way() {
        let mut e = engine();
        e.render(64);
        let neutral = e.params.value(crate::params::registry::FILTER_CUTOFF);
        e.telemetry.push_command(AudioCommand::Act(Action::SetMacro {
            macro_id: crate::core::ids::MacroId(0),
            value: -1.0,
        }));
        e.render(64);
        assert!(e.params.value(crate::params::registry::FILTER_CUTOFF) < neutral,
                "DARK did not close the filter");
    }

    #[test]
    fn the_test_tone_is_independent_of_any_voice() {
        let mut e = engine();
        e.telemetry.push_command(AudioCommand::SetTestTone(true));
        e.render(256);
        let peak = e.output()[..256].iter().fold(0.0f32, |a, b| a.max(b.abs()));
        assert!(peak > 0.01);
        assert_eq!(e.telemetry.active_voices(), 0);
    }

    #[test]
    fn active_notes_reports_the_pitch_classes_being_played() {
        let mut e = engine();
        e.telemetry.push_command(AudioCommand::Act(Action::NoteOn { note: 60, velocity: 1.0 }));
        e.telemetry.push_command(AudioCommand::Act(Action::NoteOn { note: 67, velocity: 1.0 }));
        e.render(64);
        let mask = e.active_notes();
        assert_ne!(mask & (1 << 0), 0, "C is not reported");
        assert_ne!(mask & (1 << 7), 0, "G is not reported");
        assert_eq!(mask & (1 << 1), 0, "C sharp should not be reported");
    }

    #[test]
    fn rendering_never_produces_a_nan() {
        // One NaN poisons everything downstream and the speakers go silent with
        // nothing logged anywhere.
        let mut e = engine();
        for note in 40..56u8 {
            e.telemetry.push_command(AudioCommand::Act(Action::NoteOn { note, velocity: 1.0 }));
        }
        e.telemetry.push_command(AudioCommand::Act(Action::SetMacro {
            macro_id: crate::core::ids::MacroId(1), value: 1.0,
        }));
        for _ in 0..500 {
            e.render(256);
            assert!(e.output()[..256].iter().all(|v| v.is_finite()));
        }
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib -- voice engine`
Expected: compile error — `Voice`, `VoicePool`, `ParamValues` not found.

- [ ] **Step 3: Write the voice implementation**

```rust
// src/audio/voice.rs
use crate::audio::dsp::{Adsr, AdsrParams, Osc, Svf, Waveform};
use crate::core::music::note_to_freq;
use crate::params::registry::{self as p, ParamRegistry, PARAM_COUNT};

pub const VOICE_COUNT: usize = 16;

/// A block's resolved parameter values, normalised 0..1, indexed by `ParamId`.
/// A fixed array so it can be passed into the audio path without allocating.
#[derive(Clone, Copy)]
pub struct ParamValues(pub [f32; PARAM_COUNT]);

impl Default for ParamValues {
    fn default() -> Self { ParamValues([0.0; PARAM_COUNT]) }
}

impl ParamValues {
    pub fn get(&self, id: crate::core::ids::ParamId) -> f32 { self.0[id.0 as usize] }
}

/// One sounding note.
///
/// The voice chain is written directly rather than executed through the graph.
/// Milestone M1a's patch is fixed, so per-voice graph machinery would be
/// complexity with no user-visible payoff; the module specs in `graph::patch`
/// describe this same chain, which is what the patch view draws and what M4
/// will make editable.
pub struct Voice {
    pub note: Option<u8>,
    /// Increments for every note started, so the oldest voice is identifiable.
    pub age: u64,
    velocity: f32,
    osc_a: Osc,
    osc_b: Osc,
    filter: Svf,
    env: Adsr,
    registry: ParamRegistry,
}

impl Default for Voice {
    fn default() -> Self {
        Voice {
            note: None,
            age: 0,
            velocity: 0.0,
            osc_a: Osc::default(),
            osc_b: Osc::default(),
            filter: Svf::default(),
            env: Adsr::default(),
            registry: ParamRegistry::new(),
        }
    }
}

impl Voice {
    pub fn start(&mut self, note: u8, velocity: f32, _sample_rate: f32) {
        self.note = Some(note);
        self.velocity = velocity.clamp(0.0, 1.0);
        // Oscillator phases are deliberately not reset. Restarting every voice
        // from phase zero makes stacked notes sum coherently on their first
        // cycle, which reads as a click at the start of a chord.
        self.filter.reset();
        self.env.gate_on();
    }

    pub fn release(&mut self) {
        self.env.gate_off();
    }

    pub fn is_idle(&self) -> bool {
        self.env.is_idle()
    }

    /// Adds this voice into `out`. Callers clear the buffer first.
    pub fn render(&mut self, out: &mut [f32], params: &ParamValues, sample_rate: f32) {
        if self.note.is_none() {
            return;
        }
        let note = self.note.unwrap();

        let wave = Waveform::from_index(self.registry.denormalize(p::OSC_WAVE, params.get(p::OSC_WAVE)));
        let detune = self.registry.denormalize(p::OSC_DETUNE, params.get(p::OSC_DETUNE));
        let level = self.registry.denormalize(p::OSC_LEVEL, params.get(p::OSC_LEVEL));
        let cutoff = self.registry.denormalize(p::FILTER_CUTOFF, params.get(p::FILTER_CUTOFF));
        let resonance = self.registry.denormalize(p::FILTER_RESONANCE, params.get(p::FILTER_RESONANCE));
        let adsr = AdsrParams {
            attack: self.registry.denormalize(p::ENV_ATTACK, params.get(p::ENV_ATTACK)),
            decay: self.registry.denormalize(p::ENV_DECAY, params.get(p::ENV_DECAY)),
            sustain: self.registry.denormalize(p::ENV_SUSTAIN, params.get(p::ENV_SUSTAIN)),
            release: self.registry.denormalize(p::ENV_RELEASE, params.get(p::ENV_RELEASE)),
        };

        let base = note as f32;
        let freq_a = note_to_freq(base - detune / 100.0);
        let freq_b = note_to_freq(base + detune / 100.0);
        let amp = level * self.velocity;

        for sample in out.iter_mut() {
            let env = self.env.tick(&adsr, sample_rate);
            let raw = 0.5 * (self.osc_a.tick(freq_a, sample_rate, wave)
                + self.osc_b.tick(freq_b, sample_rate, wave));
            let filtered = self.filter.lowpass(raw, cutoff, resonance, sample_rate);
            *sample += filtered * env * amp;
        }

        if self.env.is_idle() {
            self.note = None;
        }
    }
}

pub struct VoicePool {
    pub voices: [Voice; VOICE_COUNT],
    next_age: u64,
}

impl Default for VoicePool {
    fn default() -> Self {
        VoicePool { voices: std::array::from_fn(|_| Voice::default()), next_age: 0 }
    }
}

impl VoicePool {
    pub fn note_on(&mut self, note: u8, velocity: f32, sample_rate: f32) -> Option<u8> {
        self.next_age += 1;

        // Retrigger a note that is already sounding rather than spending a
        // second voice on it - otherwise a repeating key eats the whole pool.
        if let Some(v) = self.voices.iter_mut().find(|v| v.note == Some(note)) {
            v.age = self.next_age;
            v.start(note, velocity, sample_rate);
            return None;
        }

        if let Some(v) = self.voices.iter_mut().find(|v| v.note.is_none()) {
            v.age = self.next_age;
            v.start(note, velocity, sample_rate);
            return None;
        }

        // Steal the oldest. It is released rather than cut, so the theft fades
        // instead of clicking.
        let victim = self
            .voices
            .iter_mut()
            .min_by_key(|v| v.age)
            .expect("the voice pool is never empty");
        let stolen = victim.note;
        victim.age = self.next_age;
        victim.start(note, velocity, sample_rate);
        stolen
    }

    pub fn note_off(&mut self, note: u8) {
        for v in self.voices.iter_mut().filter(|v| v.note == Some(note)) {
            v.release();
        }
    }

    pub fn active_count(&self) -> usize {
        self.voices.iter().filter(|v| v.note.is_some()).count()
    }

    /// Pitch classes currently sounding, one bit per semitone, for the interface.
    pub fn active_pitch_classes(&self) -> u16 {
        self.voices.iter().filter_map(|v| v.note).fold(0u16, |m, n| m | (1 << (n % 12)))
    }

    pub fn render(&mut self, out: &mut [f32], params: &ParamValues, sample_rate: f32) {
        for v in self.voices.iter_mut() {
            v.render(out, params, sample_rate);
        }
        // Sixteen voices at full level would sum to sixteen. Scaling by the
        // square root of the count keeps a chord roughly as loud as a single
        // note without ducking audibly as notes are added.
        let active = self.active_count().max(1) as f32;
        let scale = 1.0 / active.sqrt();
        for s in out.iter_mut() {
            *s *= scale;
        }
    }
}
```

- [ ] **Step 4: Write the engine implementation**

```rust
// src/audio/engine.rs
use std::sync::Arc;

use crate::audio::voice::{ParamValues, VoicePool};
use crate::core::event::Action;
use crate::core::ids::ParamId;
use crate::engine::telemetry::{AudioCommand, EngineEvent, Telemetry};
use crate::graph::modules;
use crate::graph::module::Module;
use crate::graph::patch::{ModuleKind, Patch, PortRef};
use crate::graph::schedule::{run, BufferPool, Schedule};
use crate::params::macros::{macro_source, MACROS, MOD_SOURCE_COUNT};
use crate::params::modmatrix::{ModMatrix, ModRoute};
use crate::params::registry::{self as p, ParamRegistry, PARAM_COUNT};

/// Owns every piece of DSP state. Lives entirely on the audio thread.
pub struct AudioEngine {
    sample_rate: f32,
    pub telemetry: Arc<Telemetry>,
    pub params: ModMatrix,
    registry: ParamRegistry,

    voices: VoicePool,
    voice_bus: Vec<f32>,

    schedule: Schedule,
    pool: BufferPool,
    modules: Vec<Box<dyn Module>>,
    voice_buffer: usize,
    output_buffer: usize,
    out: Vec<f32>,

    test_tone: bool,
    test_phase: f32,
}

impl AudioEngine {
    pub fn new(sample_rate: f32, max_block: usize, telemetry: Arc<Telemetry>) -> AudioEngine {
        let registry = ParamRegistry::new();

        let mut params = ModMatrix::new(PARAM_COUNT, MOD_SOURCE_COUNT);
        for i in 0..PARAM_COUNT {
            let id = ParamId(i as u16);
            params.set_base(id, registry.normalize(id, registry.desc(id).default));
        }
        // The M1a macro routes. The remaining macros are defined but unrouted
        // until patterns exist in M5 - a knob that moves nothing is honest here,
        // and the interface marks them as inactive rather than pretending.
        let bright = macro_source(MACROS[0].id);
        let wet = macro_source(MACROS[1].id);
        let chaos = macro_source(MACROS[3].id);
        params.add_route(ModRoute { source: bright, target: p::FILTER_CUTOFF, depth: 0.45 });
        params.add_route(ModRoute { source: bright, target: p::FILTER_RESONANCE, depth: 0.15 });
        params.add_route(ModRoute { source: wet, target: p::REVERB_MIX, depth: 0.5 });
        params.add_route(ModRoute { source: wet, target: p::DELAY_MIX, depth: 0.35 });
        params.add_route(ModRoute { source: chaos, target: p::LFO_AMOUNT, depth: 0.7 });
        params.add_route(ModRoute { source: chaos, target: p::DELAY_FEEDBACK, depth: 0.3 });

        // The fixed M1a global patch. The voice chain feeds the mixer; the
        // module specs for that chain live in graph::patch so the patch view can
        // draw it even though it is executed directly in audio::voice.
        let mut patch = Patch::default();
        let mixer = patch.add(ModuleKind::Mixer);
        let delay = patch.add(ModuleKind::Delay);
        let reverb = patch.add(ModuleKind::Reverb);
        let output = patch.add(ModuleKind::Output);
        patch.connect(PortRef { module: mixer, port: 0 }, PortRef { module: delay, port: 0 })
            .expect("the built-in patch is valid");
        patch.connect(PortRef { module: delay, port: 0 }, PortRef { module: reverb, port: 0 })
            .expect("the built-in patch is valid");
        patch.connect(PortRef { module: reverb, port: 0 }, PortRef { module: output, port: 0 })
            .expect("the built-in patch is valid");

        let mut schedule = Schedule::compile(&patch).expect("the built-in patch compiles");
        let mut module_list: Vec<Box<dyn Module>> =
            patch.nodes().iter().map(|n| modules::make(n.kind)).collect();
        for m in module_list.iter_mut() {
            m.prepare(sample_rate, max_block);
        }
        // One buffer past the compiled ones carries the voice sum into the
        // mixer, whose first input the fixed patch leaves unconnected.
        let voice_buffer = schedule.buffer_count;
        let pool = BufferPool::new(schedule.buffer_count + 1, max_block);
        let mixer_step = schedule.steps.iter_mut().find(|s| s.module == mixer)
            .expect("the mixer is in the schedule");
        mixer_step.inputs[0] = Some(voice_buffer);
        let output_buffer = schedule.steps.iter().find(|s| s.module == output)
            .and_then(|s| s.outputs[0])
            .expect("the output module has an audio output port");

        AudioEngine {
            sample_rate,
            telemetry,
            params,
            registry,
            voices: VoicePool::default(),
            voice_bus: vec![0.0; max_block],
            schedule,
            pool,
            modules: module_list,
            voice_buffer,
            output_buffer,
            out: vec![0.0; max_block],
            test_tone: false,
            test_phase: 0.0,
        }
    }

    fn drain_commands(&mut self) {
        // Drain everything waiting, not one per block: sixteen notes pushed
        // together must all sound in the same block, or a chord spreads out.
        while let Some(cmd) = self.telemetry.commands.pop() {
            match cmd {
                AudioCommand::SetTestTone(on) => self.test_tone = on,
                AudioCommand::Act(action) => match action {
                    Action::NoteOn { note, velocity } => {
                        if let Some(stolen) = self.voices.note_on(note, velocity, self.sample_rate) {
                            self.telemetry.push_event(EngineEvent::VoiceStolen { note: stolen });
                        }
                        self.telemetry.push_event(EngineEvent::NoteStarted { note });
                    }
                    Action::NoteOff { note } => {
                        self.voices.note_off(note);
                        self.telemetry.push_event(EngineEvent::NoteEnded { note });
                    }
                    Action::SetMacro { macro_id, value } => {
                        self.params.set_source(macro_source(macro_id), value);
                    }
                    Action::SetParam { target, value } => {
                        self.params.set_base(target, value);
                    }
                    // Transport, octave and velocity are handled on the input
                    // thread; they never reach the audio thread in M1a.
                    Action::Transport(_) | Action::OctaveShift(_) | Action::VelocityShift(_) => {}
                },
            }
        }
    }

    pub fn render(&mut self, frames: usize) {
        let frames = frames.min(self.out.len());
        self.drain_commands();
        self.params.recompute();

        let mut values = ParamValues::default();
        values.0.copy_from_slice(self.params.values());

        self.voice_bus[..frames].fill(0.0);
        self.voices.render(&mut self.voice_bus[..frames], &values, self.sample_rate);

        if self.test_tone {
            let inc = 440.0 / self.sample_rate;
            for s in self.voice_bus[..frames].iter_mut() {
                *s += (self.test_phase * std::f32::consts::TAU).sin() * 0.2;
                self.test_phase += inc;
                if self.test_phase >= 1.0 {
                    self.test_phase -= 1.0;
                }
            }
        }

        // Hand the voice sum to the mixer through the reserved buffer, run the
        // global graph, then take the terminal node's audio back out.
        self.pool.write(self.voice_buffer, &self.voice_bus[..frames]);

        run(&self.schedule, &mut self.pool, &mut self.modules, frames,
            self.sample_rate, self.params.values());

        self.out[..frames].copy_from_slice(&self.pool.buffer(self.output_buffer)[..frames]);
        self.telemetry.set_peak(
            self.out[..frames].iter().fold(0.0f32, |a, b| a.max(b.abs())),
        );

        self.telemetry.set_active_voices(self.voices.active_count() as u32);
    }

    pub fn output(&self) -> &[f32] {
        &self.out
    }

    pub fn active_notes(&self) -> u16 {
        self.voices.active_pitch_classes()
    }
}
```

**Two supporting changes this needs.**

1. `BufferPool` gains a write method, so the engine can hand the voice sum in:

```rust
// add to BufferPool in src/graph/schedule.rs
pub fn write(&mut self, index: usize, src: &[f32]) {
    self.bufs[index][..src.len()].copy_from_slice(src);
}
```

2. `ModuleKind::Output` needs an **audio output port** so the terminal node's
   signal lands in a pool buffer the engine can read. Downcasting a
   `Box<dyn Module>` to reach `Output::buffer` would need `Any` and is not worth
   it. In Task 9's `spec_for`, change the Output arm to:

```rust
ModuleKind::Output => &ModuleSpec {
    name: "OUTPUT", zone: Zone::Global,
    inputs: ports!("audio": Audio),
    outputs: ports!("audio": Audio),   // was empty
    params: &[p::MASTER_GAIN],
},
```

and have `Output::process` write the gained signal to `ctx.output(0)` as well as
recording the peak. No Task 9 test depends on Output having no outputs — the
type-mismatch test uses the LFO — so the suite should stay green. Run
`cargo test --lib patch` after the change to confirm that rather than assume it.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib -- voice engine`
Expected: 16 passing tests.
Run: `cargo run`, open DIAGNOSTICS, tick the test tone — still audible, and **Active voices** reads 0 while it sounds.

- [ ] **Step 6: Mutation-check the voice pool and the drain**

1. Change `drain_commands` to `if let Some(cmd) = ... ` instead of `while let`.
   Expected: `commands_are_drained_every_block_not_one_per_block` fails. Restore.
2. Remove the retrigger branch from `note_on` so a sounding note allocates a second voice.
   Expected: `retriggering_a_sounding_note_reuses_its_voice` fails. Restore.
3. Change `min_by_key(|v| v.age)` to `max_by_key`.
   Expected: `the_pool_steals_the_oldest_voice_when_full` fails. Restore.
4. Remove the `scale` multiplication in `VoicePool::render`.
   Expected: `sixteen_simultaneous_voices_stay_in_range` fails. Restore.

- [ ] **Step 7: Commit**

```bash
git add src/audio src/graph
git commit -m "feat(audio): sixteen voices feeding the scheduled global graph

Commands drain fully every block rather than one per block - a chord of
sixteen notes pushed together must sound together, not spread over a
quarter of a second. Pinned, because the one-per-block version sounds
almost right and is therefore easy to miss.

The pool retriggers a note that is already sounding rather than spending a
second voice, or a repeating key would eat all sixteen. Stealing takes the
oldest voice and releases it rather than cutting it, so the theft fades
instead of clicking. Voice sum is scaled by the square root of the active
count, which keeps a chord about as loud as a single note without ducking
audibly as notes are added.

Oscillator phase is deliberately not reset on note start: restarting every
voice at phase zero makes stacked notes sum coherently on their first
cycle, which reads as a click at the top of a chord.

The voice chain is written directly rather than run through the graph. M1a's
patch is fixed, so per-voice graph machinery would be complexity with no
visible payoff; the module specs describe the same chain, so the patch view
draws the truth and M4 can make it editable without changing the contract."
```

---

## Task 14: Keyboard input — playable

**Files:**
- Create: `src/input/keyboard.rs`
- Modify: `src/input/mod.rs`, `src/app.rs`, `src/ui/performance.rs`

**Interfaces:**
- Consumes: `KeyCode`, `ControlId`, `ControlValue`, `ControlEvent`, `Action` (Task 5); `Mapping`, `Binding`, `HeldSet`, `PlayState`, `resolve` (Task 6); `Telemetry`, `AudioCommand` (Task 12)
- Produces:
  - `from_egui(key: egui::Key) -> Option<KeyCode>`
  - `default_mapping() -> Mapping`
  - `KeyboardSource { held: HeldSet, play: PlayState }` with `pump(&mut self, ctx: &egui::Context, mapping: &Mapping, telemetry: &Telemetry)`

- [ ] **Step 1: Write the failing tests**

```rust
// src/input/keyboard.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::event::{Action, ControlId, ControlValue, ControlEvent, KeyCode};
    use crate::input::mapping::{resolve, HeldSet, PlayState};

    fn press(key: KeyCode) -> ControlEvent {
        ControlEvent { id: ControlId::Keyboard(key), value: ControlValue::Gate(true), host_time_ns: 0 }
    }

    #[test]
    fn the_home_row_plays_a_chromatic_scale_from_c() {
        let m = default_mapping();
        let play = PlayState { octave: 4, velocity: 1.0 };
        let expected = [
            (KeyCode::A, 60), (KeyCode::W, 61), (KeyCode::S, 62), (KeyCode::E, 63),
            (KeyCode::D, 64), (KeyCode::F, 65), (KeyCode::T, 66), (KeyCode::G, 67),
            (KeyCode::Y, 68), (KeyCode::H, 69), (KeyCode::U, 70), (KeyCode::J, 71),
            (KeyCode::K, 72),
        ];
        for (key, note) in expected {
            let out = resolve(&m, &HeldSet::default(), &play, &press(key));
            assert_eq!(out, vec![Action::NoteOn { note, velocity: 1.0 }], "{key:?} played the wrong note");
        }
    }

    #[test]
    fn the_black_keys_sit_where_a_piano_would_put_them() {
        // W, E, T, Y, U are the sharps. If this drifts, the layout stops being
        // learnable by anyone who has seen a keyboard.
        let m = default_mapping();
        let play = PlayState { octave: 4, velocity: 1.0 };
        for key in [KeyCode::W, KeyCode::E, KeyCode::T, KeyCode::Y, KeyCode::U] {
            let out = resolve(&m, &HeldSet::default(), &play, &press(key));
            let Action::NoteOn { note, .. } = out[0] else { panic!("expected a note") };
            let pitch_class = note % 12;
            assert!([1, 3, 6, 8, 10].contains(&pitch_class),
                    "{key:?} produced pitch class {pitch_class}, which is a white key");
        }
    }

    #[test]
    fn the_transport_and_octave_keys_are_bound() {
        let m = default_mapping();
        let play = PlayState::default();
        let space = resolve(&m, &HeldSet::default(), &play, &press(KeyCode::Space));
        assert_eq!(space, vec![Action::Transport(crate::core::event::TransportCmd::Toggle)]);
        let down = resolve(&m, &HeldSet::default(), &play, &press(KeyCode::Z));
        assert_eq!(down, vec![Action::OctaveShift(-1)]);
        let up = resolve(&m, &HeldSet::default(), &play, &press(KeyCode::X));
        assert_eq!(up, vec![Action::OctaveShift(1)]);
    }

    #[test]
    fn no_key_is_bound_twice() {
        // Two meanings on one key is a mapping bug that presents as a mysterious
        // extra note rather than as an error.
        let m = default_mapping();
        for (id, bindings) in &m.bindings {
            assert_eq!(bindings.len(), 1, "{id:?} has {} bindings", bindings.len());
        }
    }

    #[test]
    fn octave_changes_are_clamped_to_a_playable_range() {
        let mut s = KeyboardSource::default();
        for _ in 0..50 { s.apply_local(Action::OctaveShift(1)); }
        assert!(s.play.octave <= 8, "octave ran away to {}", s.play.octave);
        for _ in 0..50 { s.apply_local(Action::OctaveShift(-1)); }
        assert!(s.play.octave >= -1, "octave ran away to {}", s.play.octave);
    }

    #[test]
    fn velocity_changes_stay_within_the_usable_range() {
        let mut s = KeyboardSource::default();
        for _ in 0..50 { s.apply_local(Action::VelocityShift(0.1)); }
        assert!(s.play.velocity <= 1.0);
        for _ in 0..50 { s.apply_local(Action::VelocityShift(-0.1)); }
        assert!(s.play.velocity >= 0.1, "velocity reached {} - silent keys look broken", s.play.velocity);
    }

    #[test]
    fn actions_handled_locally_are_not_forwarded_to_audio() {
        // Octave and velocity live on the input thread. Sending them to the
        // engine would be a command it must ignore, and a reader would wonder why.
        assert!(!KeyboardSource::goes_to_audio(&Action::OctaveShift(1)));
        assert!(!KeyboardSource::goes_to_audio(&Action::VelocityShift(0.1)));
        assert!(KeyboardSource::goes_to_audio(&Action::NoteOn { note: 60, velocity: 1.0 }));
        assert!(KeyboardSource::goes_to_audio(&Action::SetMacro {
            macro_id: crate::core::ids::MacroId(0), value: 0.5 }));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib keyboard`
Expected: compile error — `default_mapping`, `KeyboardSource` not found.

- [ ] **Step 3: Write the implementation**

```rust
// src/input/keyboard.rs
use crate::core::event::{Action, ControlEvent, ControlId, ControlValue, KeyCode, TransportCmd};
use crate::engine::telemetry::{AudioCommand, Telemetry};
use crate::input::mapping::{resolve, Binding, HeldSet, Mapping, PlayState};

/// Translate a windowing key into a FLUX key.
///
/// This is the only place `egui::Key` appears outside the interface, which is
/// what keeps `core` free of interface crates.
pub fn from_egui(key: egui::Key) -> Option<KeyCode> {
    use egui::Key as E;
    Some(match key {
        E::A => KeyCode::A, E::B => KeyCode::B, E::C => KeyCode::C, E::D => KeyCode::D,
        E::E => KeyCode::E, E::F => KeyCode::F, E::G => KeyCode::G, E::H => KeyCode::H,
        E::I => KeyCode::I, E::J => KeyCode::J, E::K => KeyCode::K, E::L => KeyCode::L,
        E::M => KeyCode::M, E::N => KeyCode::N, E::O => KeyCode::O, E::P => KeyCode::P,
        E::Q => KeyCode::Q, E::R => KeyCode::R, E::S => KeyCode::S, E::T => KeyCode::T,
        E::U => KeyCode::U, E::V => KeyCode::V, E::W => KeyCode::W, E::X => KeyCode::X,
        E::Y => KeyCode::Y, E::Z => KeyCode::Z,
        E::Num1 => KeyCode::Num1, E::Num2 => KeyCode::Num2, E::Num3 => KeyCode::Num3,
        E::Num4 => KeyCode::Num4, E::Num5 => KeyCode::Num5, E::Num6 => KeyCode::Num6,
        E::Num7 => KeyCode::Num7, E::Num8 => KeyCode::Num8, E::Num9 => KeyCode::Num9,
        E::Num0 => KeyCode::Num0,
        E::Space => KeyCode::Space,
        E::Comma => KeyCode::Comma, E::Period => KeyCode::Period,
        E::Minus => KeyCode::Minus, E::Plus => KeyCode::Plus,
        E::Slash => KeyCode::Slash, E::Backslash => KeyCode::Backslash,
        _ => return None,
    })
}

/// The layout is chosen by physical position, so it behaves the same on QWERTZ
/// and QWERTY: the lower row is the white keys, the upper row the black ones,
/// arranged as they sit on a piano.
const NOTE_KEYS: [(KeyCode, i8); 13] = [
    (KeyCode::A, 0),  (KeyCode::W, 1),  (KeyCode::S, 2),  (KeyCode::E, 3),
    (KeyCode::D, 4),  (KeyCode::F, 5),  (KeyCode::T, 6),  (KeyCode::G, 7),
    (KeyCode::Y, 8),  (KeyCode::H, 9),  (KeyCode::U, 10), (KeyCode::J, 11),
    (KeyCode::K, 12),
];

pub fn default_mapping() -> Mapping {
    let mut m = Mapping::default();
    for (key, semitone) in NOTE_KEYS {
        m.insert(ControlId::Keyboard(key), Binding::Note { semitone });
    }
    m.insert(ControlId::Keyboard(KeyCode::Z), Binding::Act(Action::OctaveShift(-1)));
    m.insert(ControlId::Keyboard(KeyCode::X), Binding::Act(Action::OctaveShift(1)));
    m.insert(ControlId::Keyboard(KeyCode::C), Binding::Act(Action::VelocityShift(-0.1)));
    m.insert(ControlId::Keyboard(KeyCode::V), Binding::Act(Action::VelocityShift(0.1)));
    m.insert(ControlId::Keyboard(KeyCode::Space), Binding::Act(Action::Transport(TransportCmd::Toggle)));
    m
}

#[derive(Default)]
pub struct KeyboardSource {
    pub held: HeldSet,
    pub play: PlayState,
}

impl KeyboardSource {
    /// Whether an action belongs to the audio thread. Octave and velocity are
    /// input-thread state; forwarding them would be a command the engine has to
    /// ignore, and a reader would rightly wonder why.
    pub fn goes_to_audio(action: &Action) -> bool {
        !matches!(action, Action::OctaveShift(_) | Action::VelocityShift(_))
    }

    pub fn apply_local(&mut self, action: Action) {
        match action {
            // Clamped: an octave beyond hearing and a velocity of zero both
            // present as "the keyboard stopped working".
            Action::OctaveShift(d) => self.play.octave = (self.play.octave + d).clamp(-1, 8),
            Action::VelocityShift(d) => self.play.velocity = (self.play.velocity + d).clamp(0.1, 1.0),
            _ => {}
        }
    }

    /// Read this frame's key events and turn them into engine commands.
    pub fn pump(&mut self, ctx: &egui::Context, mapping: &Mapping, telemetry: &Telemetry) {
        let events: Vec<(egui::Key, bool, bool)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, pressed, repeat, .. } => Some((*key, *pressed, *repeat)),
                    _ => None,
                })
                .collect()
        });

        for (key, pressed, repeat) in events {
            // The operating system repeats a held key. A synth must not restart
            // the note; the key is still down and the note is still sounding.
            if repeat {
                continue;
            }
            let Some(code) = from_egui(key) else { continue };
            let id = ControlId::Keyboard(code);

            if pressed {
                self.held.press(id);
            } else {
                self.held.release(id);
            }

            let event = ControlEvent {
                id,
                value: ControlValue::Gate(pressed),
                host_time_ns: 0,
            };
            for action in resolve(mapping, &self.held, &self.play, &event) {
                if Self::goes_to_audio(&action) {
                    telemetry.push_command(AudioCommand::Act(action));
                } else {
                    self.apply_local(action);
                }
            }
        }
    }
}
```

Add `pub mod keyboard;` to `src/input/mod.rs`.

Wire it into `FluxApp::update`, before the panels are drawn:

```rust
if let Some(host) = &self.host {
    self.keyboard.pump(ctx, &self.mapping, &host.telemetry);
}
```

And show live state in the performance view:

```rust
// src/ui/performance.rs
use crate::ui::theme;

const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

pub fn show(ui: &mut egui::Ui, active: u16, octave: i8, velocity: f32, peak: f32, voices: u32) {
    ui.heading("FLUX");
    ui.add_space(12.0);

    ui.horizontal(|ui| {
        for (i, name) in NOTE_NAMES.iter().enumerate() {
            let lit = active & (1 << i) != 0;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(46.0, 96.0), egui::Sense::hover());
            let fill = if lit { theme::ACCENT } else { theme::SURFACE_HI };
            ui.painter().rect_filled(rect, theme::R_SM, fill);
            ui.painter().text(
                rect.center_bottom() - egui::vec2(0.0, 14.0),
                egui::Align2::CENTER_CENTER,
                name,
                egui::FontId::monospace(12.0),
                if lit { theme::BG } else { theme::TEXT_DIM },
            );
        }
    });

    ui.add_space(16.0);
    ui.horizontal(|ui| {
        ui.label(format!("OCTAVE {octave}"));
        ui.separator();
        ui.label(format!("VELOCITY {:.0}%", velocity * 100.0));
        ui.separator();
        ui.label(format!("VOICES {voices}"));
    });

    ui.add_space(8.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(240.0, 8.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, theme::R_SM, theme::SURFACE_HI);
    let filled = egui::Rect::from_min_size(
        rect.min,
        egui::vec2(rect.width() * peak.clamp(0.0, 1.0), rect.height()),
    );
    let colour = if peak > 0.95 { theme::DANGER } else { theme::ACCENT };
    ui.painter().rect_filled(filled, theme::R_SM, colour);

    ui.add_space(20.0);
    ui.small("A W S E D F T G Y H U J K play.  Z X change octave.  C V change velocity.");
}
```

- [ ] **Step 4: Run the tests and play it**

Run: `cargo test --lib keyboard` — expected: 7 passing tests.
Run: `cargo test` — expected: the whole suite green.
Run: `cargo run` and:
1. Press `A` — a note sounds and the `C` block lights up.
2. Hold `A` — the note sustains and does **not** retrigger or stutter.
3. Press `A S D G` together — a chord sounds; `VOICES` reads 4.
4. Press `X` then `A` — the same key plays an octave higher.
5. Hold ten keys at once — no crackle, no dropout; `Dropped commands` stays at 0 in DIAGNOSTICS.

If step 2 stutters, the `repeat` filter is not working. If step 5 drops commands, raise the queue capacity in `Telemetry::new` — but first check that `drain_commands` really uses `while let`.

- [ ] **Step 5: Mutation-check the key repeat filter and the local/audio split**

1. Remove the `if repeat { continue; }` guard.
   Expected: no unit test fails — this one is only observable by ear, and the manual check in Step 4.2 is the real test. Restore it and say so in the commit message rather than pretending a unit test covers it.
2. Change `goes_to_audio` to always return `true`.
   Expected: `actions_handled_locally_are_not_forwarded_to_audio` fails. Restore.
3. Remove the `.clamp(-1, 8)` from `OctaveShift`.
   Expected: `octave_changes_are_clamped_to_a_playable_range` fails. Restore.

- [ ] **Step 6: Commit**

```bash
git add src/input src/ui src/app.rs
git commit -m "feat(input): computer keyboard - FLUX is playable

Press a key, hear a synth, see the note. The milestone this plan exists for.

The layout is chosen by physical position so it behaves the same on QWERTZ
and QWERTY, with the upper row as the black keys where a piano would put
them - a test pins that, because once it drifts the layout stops being
learnable by anyone who has seen a keyboard.

egui::Key appears only in this file. That is what keeps core free of
interface crates, and it is the same seam MIDI and the guitar will enter
through in M2 and M3.

Octave and velocity are input-thread state and are not forwarded to the
engine; sending them would be a command it has to ignore. Both are clamped,
because an octave beyond hearing and a velocity of zero both present as
'the keyboard stopped working'.

Mutation-checking found an honest gap: removing the key-repeat filter
breaks no unit test, because the operating system's repeat only exists in a
real window. The manual check in the plan is the real test for it."
```

---

## Definition of Done for this plan

```
cargo test                          all green
cargo clippy -- -D warnings         clean
cargo run
        ↓
a dark window opens
        ↓
press A                             a synth note sounds immediately
        ↓
the C block lights up
        ↓
press A S D G together              a chord; VOICES reads 4
        ↓
hold a key                          it sustains without stuttering
        ↓
press X, press A                    the same key, an octave higher
        ↓
DIAGNOSTICS                         real latency, 0 dropped, 0 underruns
```

The next plan (M1b) adds the transport, the click, the event looper and the performance view's loop lanes, completing milestone M1 and the brief's §39.
