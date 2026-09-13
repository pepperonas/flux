# FLUX — Controllers

How physical hardware becomes FLUX control events, and back again.

Every device is described by a **profile**: data keyed by USB or MIDI identity,
not code. A profile says which physical control carries which `ControlId`, and
how FLUX state is rendered back to the device. Unknown hardware is handled by the
same learning wizard rather than by a code change.

> **On verification.** Each fact below is marked ✅ verified, 📄 from the
> manufacturer's reference, or ❓ assumed. Anything ❓ is checked against the
> physical device before it is relied on. Numbers in this project are measured,
> not remembered.

---

## Novation Launchkey Mini 25 MK4

Primary controller: 25 velocity keys, 16 RGB pads with polyphonic aftertouch,
8 endless encoders, OLED display, transport and navigation buttons.

Source: [Launchkey MK4 Programmer's Reference Guide v2.0](https://fael-downloads-prod.focusrite.com/customer/prod/downloads/launchkey_mk4_programmer_s_reference_guide_v2_en.pdf).

### Ports and lifecycle

📄 The device presents **two port pairs**: `MIDI In/Out` for performance data and
`DAW In/Out` for surface control. Everything below happens on the DAW port.

| Action | Message | |
|---|---|---|
| Enter DAW mode | `9Fh 0Ch 7Fh` | on connect |
| Leave DAW mode | `9Fh 0Ch 00h` | on exit — **including on panic** |
| Take over drum pads | `B6h 54h 01h` | required before pad LEDs respond |
| Release drum pads | `B6h 54h 00h` | |

Leaving DAW mode on shutdown is not politeness. A device left in DAW mode
behaves strangely in whatever the user opens next.

### Pads — 16, RGB, polyphonic aftertouch

📄 In DAW-controlled drum mode the pads report on **channel 10**: note on/off
(`9Ah`) and polyphonic aftertouch (`AAh`). The pads use force-sensing resistors,
so aftertouch is continuous per pad — each pad is both a trigger *and* a
continuous modulation source, which is why `ControlValue` needs both shapes.

LED colour is written back on:

| Channel | Effect |
|---|---|
| 10 | static colour |
| 11 | flashing colour |
| 12 | pulsing colour |

📄 Colour is a palette index in the velocity/value byte. True RGB is available by
SysEx — **the Mini SKU uses `02h 13h`, not the `02h 14h` of the full-size
models**:

```
F0h 00h 20h 29h 02h 13h 01h 43h <padID> <R> <G> <B> F7h
```

📄 **Flashing and pulsing are synchronised to MIDI beat clock by the device**
(flash = one beat, pulse = two beats; it falls back to 120 BPM if no clock
arrives). This is worth more than it first appears: if FLUX sends MIDI clock, a
recording pad blinks *in time with the music* with no timing code on our side.
It also gives MIDI clock output a real consumer, which is why it is no longer
deferred.

Planned FLUX layout ❓ (to be confirmed once the device is in hand):

```
row 1   loop tracks / scene launch     dark = empty · dim = has content
                                       green pulse = playing · red flash = armed
row 2   drum voices / pattern triggers colour by voice, lit on hit
```

### Encoders — 8, endless

The current input implementation accepts CC **21–28** as a provisional FLUX
macro mapping (CC 21 → BRIGHT, through CC 28 → SPACE) on any channel. This is
an implementation convenience, not a verified MK4 protocol claim; the exact
per-control indices remain ❓ until they are observed from the physical unit.

The input implementation also accepts a provisional channel-10 pad layout:
notes 36–39 select loop tracks 1–4, 40 toggles recording, 41 clears, 42 undoes,
43 mutes, and 44–46 toggle, stop, and start transport. These actions are
intentionally input-only; pad LED feedback and the remaining pad assignments
require physical-device verification.

When a MIDI output whose name contains `DAW` is available, FLUX sends the
documented DAW-mode and drum-pad takeover messages on connect. Loop-track and
transport state is then sent as channel-10 pad note messages, throttled to state
changes. On shutdown FLUX releases drum-pad takeover and leaves DAW mode.
OLED field indices and physical RGB rendering remain unverified. FLUX sends
standardized RGB SysEx colors for the four loop pads when a DAW output exists.
FLUX also emits MIDI clock (`F8`) at 24 pulses per quarter note while transport
is running, based on the active sample rate and BPM. Transport transitions emit
MIDI Start (`FA`) and Stop (`FC`); restarting resets the clock position.
On DAW connection it configures the documented stationary OLED arrangement 3
and writes the title `FLUX`; per-control OLED fields remain pending hardware
verification.

📄 Absolute CC on channel 16 in Plugin/Mixer/Sends modes; **relative** output in
Transport mode with pivot `40h` — `41h` is one step clockwise, `3Fh` one step
anticlockwise. 📄 The host can also switch encoders to relative output through
the feature controls.

**FLUX drives them relative.** Absolute mode means that switching macro pages
re-points a knob at a different parameter whose value does not match the knob's
last position, and the next touch jumps the sound. Relative cannot jump by
construction. The readout goes to the screen instead, where it belongs.

📄 Touch is reported separately on **channel 15** (127 on touch, 0 on release),
*before* any value change. FLUX uses this to show the parameter name the moment a
finger lands — you see what you are about to change before you change it.

The eight encoders carry the eight macros: `BRIGHT ↔ DARK`, `WET ↔ DRY`,
`ENERGY`, `CHAOS`, `DENSITY`, `SPACE`. Opposing pairs are one bipolar axis with
the displayed name following the sign — endless encoders have no end stop, so a
centred axis is the control they are physically built for.

### Screen

📄 Host-writable. Display arrangement **ID 3 is "1 line + 2×4: Title and 8 names
(for encoder designations)"** — literally the macro page, labelled, on the
device. Arrangements 1, 2 and 4 give name/value readouts; custom bitmaps are also
possible.

```
configure   F0h 00h 20h 29h 02h 13h 04h <target> <config> F7h
set text    F0h 00h 20h 29h 02h 13h 06h <target> <field> <text…> F7h
```

📄 Targets include per-control temporary displays, a stationary display (`20h`), a
global temporary display (`21h`), and the names shown for each pad/encoder mode.
Text is ASCII `20h`–`7Eh` plus four reassigned control codes (empty box, filled
box, flat symbol, heart).

### Device features

📄 Settable on **channel 7**, queryable on channel 8, on the DAW in port:
scale behaviour, scale tonic and scale type, the full arpeggiator, keyboard
zones, encoder relative output.

**FLUX owns key and scale and pushes them to the device.** Changing the key in
FLUX reconfigures the keyboard's own scale mode, so notes outside the key stop
responding *on the hardware*. That is the brief's musical safety requirement
made physical — unreachable from software alone.

The arpeggiator is left to the device by default. It produces ordinary notes,
FLUX sees ordinary notes, and nothing needs to be built.

### Not yet extracted ❓

The per-control CC and note indices, and the colour palette table, are **images**
in the reference PDF and did not survive text extraction. The feature-control
table extracted with its columns visibly misaligned and is therefore not quoted
here at all. All of these are read off the hardware directly when it arrives —
which is more reliable than the document anyway.

---

## Guitar Hero X-plorer (Xbox 360)

✅ Measured on the connected unit, not assumed:

```
VID 0x1430 (RedOctane Inc(c)2006)   PID 0x4748   serial 075E517
bDeviceClass 255 (vendor-specific)
Interface 0   0xFF / 0x5D / 0x01   2 endpoints   ← XInput, the one we read
Interface 1   0xFF / 0x5D / 0x03   4 endpoints   (headset)
Interface 2   0xFF / 0x5D / 0x02   1 endpoint
Interface 3   0xFF / 0xFD / 0x13   0 endpoints   (security)
```

✅ The device appears under `IOUSB` but **not** under `IOHIDDevice`. This is by
design — Xbox 360 controllers are vendor-specific, not HID — and it means **no
HID library can ever see this device on macOS**. FLUX reads interrupt transfers
from interface 0 over raw USB via `rusb`. No driver or kernel extension.

✅ libusb can claim interface 0. Verified 2026-09-09 with a throwaway `rusb`
probe (outside the repository, not kept) against the physically connected unit
on this Mac (confirmed via `ioreg -p IOUSB -l -w 0` before and after). No kernel
driver was attached (`kernel_driver_active` → `false`, matching the
`IOCFPlugInTypes`/generic-`IOUSBLib` observation above). `open()`,
`claim_interface(0)` and `release_interface(0)` all returned `Ok`, with no
`Access denied` or `Resource busy` error and no macOS permission prompt. Result
reproduced identically across two independent runs. Observed IN endpoint
`0x81`, max packet size 32 bytes; actual interrupt reports read back at 20
bytes. This clears the only open question for M3 — raw USB access to this
device from user-space on macOS works as planned.

❓ Which report byte carries which fret, and which axis is whammy versus tilt.
These are **not guessed**. The setup wizard diffs incoming reports while the user
presses each control in turn, which also covers units that deviate from the
documented layout. The probe run above captured one static 20-byte report
(`00 14 00 00 87 7a 00 00 00 00 00 80 ff 7f 00 00 00 00 00 00`) while no one was
touching the controller — its resting/idle state, not a control identification.
No control was pressed during the probe (run unattended), so no byte-to-control
mapping was learned; that remains entirely the setup wizard's job.

The Diagnostics view now exposes the latest report bytes and a 32-bit changed-byte
mask. This makes each physical movement observable before assigning a control
name and keeps the mapping evidence separate from the audio path.

Controls: 5 frets · strum up/down · whammy · tilt · start · select · d-pad.
It has no feedback, so its `render` is a no-op.

---

## Computer keyboard

Always available, needs no hardware, and must stay fully playable — it is the
zero-setup path and the fallback when nothing else is connected.

```
A W S E D F T G Y H U J K     chromatic from C
Z / X                          octave down / up
C / V                          velocity down / up
1 – 4                          select loop track
Space                          transport play / stop
R                              loop record arm / toggle
?                              help
```

Layout is physical-position based, so it behaves the same on QWERTZ and QWERTY.

---

## The two devices together

The guitar and the Launchkey are not competing for the same role, and FLUX does
not ask you to choose:

* **Launchkey** — notes, state and parameters. What is playing, what is
  recording, what a macro is set to. It shows you the state of the instrument.
* **Guitar** — gesture and expression. Whammy, tilt, strum: continuous, physical,
  two-handed, and nothing a keyboard can imitate.

Playing both at once is the intended shape of a FLUX performance, and the reason
the control layer never learns which device an event came from.
